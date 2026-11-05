use super::*;
#[test]
fn only_sdk_private_desktop_names() {
    for count in [31, 32] {
        assert!(private_desktop(&format!(
            "CodexSandboxDesktop-{}",
            "a".repeat(count)
        )));
    }
    for name in [
        "Default",
        "WinSta0",
        "CodexSandboxDesktop-",
        "CodexSandboxDesktop-123456789abcdef0123456789abcdefg",
    ] {
        assert!(!private_desktop(name));
    }
}
#[test]
fn only_unprotected_null_label_is_absent() {
    let absent = parse("D:(A;;FA;;;WD)").unwrap();
    let null = parse("D:(A;;FA;;;WD)S:NO_ACCESS_CONTROL").unwrap();
    assert_eq!(stringify(absent.0).unwrap(), stringify(null.0).unwrap());
    for sacl in [
        "PNO_ACCESS_CONTROL",
        "AINO_ACCESS_CONTROL",
        "ARNO_ACCESS_CONTROL",
        "(ML;;NW;;;ME)",
    ] {
        let other = parse(&format!("D:(A;;FA;;;WD)S:{sacl}")).unwrap();
        assert_ne!(stringify(absent.0).unwrap(), stringify(other.0).unwrap());
    }
}
// R9: preserve stored legacy/protected/inherited ACLs and leave unjournaled children untouched.
#[test]
fn native_identity_label_dacl_roundtrip() {
    for control in [None, Some(0), Some(0x1000)] {
        native_identity_label_dacl_case(control);
    }
}
fn native_identity_label_dacl_case(initial_control: Option<u16>) {
    let root = std::env::temp_dir().join(format!(
        "taskq-security-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    if let Some(control) = initial_control {
        let opened = open(&root, true).unwrap();
        let saved = Snapshot::from_open(&root, &opened).unwrap();
        let storage = parse(&saved.descriptor).unwrap();
        let mut descriptor = components(storage.0).unwrap();
        descriptor.control = (descriptor.control & !0x1500) | control;
        let count = unsafe { *descriptor.dacl.cast::<u16>().add(2) };
        for index in 0..u32::from(count) {
            let mut ace = ptr::null_mut();
            boolean(
                unsafe { GetAce(descriptor.dacl, index, &mut ace) },
                "read fixture ACE",
            )
            .unwrap();
            unsafe {
                *ace.cast::<u8>().add(1) &= !0x10;
            }
        }
        let result = unsafe {
            NtSetSecurityObject(
                opened.file.0,
                4,
                (&mut descriptor as *mut Descriptor).cast(),
            )
        };
        assert!(result >= 0, "legacy fixture creation failed: {result:#x}");
    }
    let file = root.join("owner.txt");
    std::fs::write(&file, "before").unwrap();
    let original = Snapshot::capture(&root).unwrap();
    let child_original = Snapshot::capture(&file).unwrap();
    if let Some(control) = initial_control {
        let storage = parse(&original.descriptor).unwrap();
        assert_eq!(
            components(storage.0).unwrap().control & 0x1500,
            control,
            "fixture inheritance state"
        );
    }
    let low = original.low().unwrap();
    original.apply(&low).unwrap();
    assert_eq!(low.low().unwrap(), low, "preparation must be idempotent");
    assert_eq!(
        Snapshot::capture(&file).unwrap(),
        child_original,
        "no unjournaled child propagation"
    );
    std::fs::write(&file, "owner write").unwrap();
    std::fs::remove_file(&file).unwrap();
    let new_file = root.join("new.txt");
    std::fs::write(&new_file, "owner create")
        .expect("owner can create a file in prepared directory");
    std::fs::remove_file(&new_file).expect("owner can delete a newly created file");
    low.apply(&original).unwrap();
    assert_eq!(Snapshot::capture(&root).unwrap(), original);
    std::fs::write(&file, "identity replacement").unwrap();
    assert!(
        child_original
            .apply(&child_original.medium().unwrap())
            .is_err()
    );
    let current = Snapshot::capture(&file).unwrap();
    let medium = current.medium().unwrap();
    current.apply(&medium).unwrap();
    assert!(
        current.apply(&current.low().unwrap()).is_err(),
        "concurrent label change must refuse"
    );
    medium.apply(&current).unwrap();
    assert_eq!(Snapshot::capture(&file).unwrap(), current);
    use std::os::windows::fs::OpenOptionsExt;
    let live_owner_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(7)
        .open(&file)
        .unwrap();
    let current = Snapshot::capture(&file).unwrap();
    let changed_acl = current.deny(0x40, "S-1-1-0").unwrap();
    assert_eq!(changed_acl.deny(0x40, "S-1-1-0").unwrap(), changed_acl);
    current.apply(&changed_acl).unwrap();
    assert!(
        current.apply(&current.low().unwrap()).is_err(),
        "concurrent DACL change must refuse"
    );
    changed_acl.apply(&current).unwrap();
    drop(live_owner_file);
    let junction = root.join("junction");
    assert!(
        std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&root)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(Snapshot::capture(&junction).is_err());
    assert!(Snapshot::capture(&junction.join("owner.txt")).is_err());
    assert!(create_directory(&junction.join("must-not-exist")).is_err());
    assert!(
        !root.join("must-not-exist").exists(),
        "directory creation must not follow a junction"
    );
    std::fs::remove_dir(&junction).unwrap();
    std::fs::remove_file(&file).unwrap();
    std::fs::remove_dir(&root).unwrap();
}
