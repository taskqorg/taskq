use super::*;

// Chain: M14 Windows permission. Risk: unknown or denied permission becomes healthy and posts, or a fresh host never posts.
#[test]
fn permission_denial_and_errors() {
    assert_eq!(setting(Ok(NotificationSetting::Enabled)), "allowed");
    for s in [
        NotificationSetting::DisabledForApplication,
        NotificationSetting::DisabledForUser,
        NotificationSetting::DisabledByGroupPolicy,
        NotificationSetting::DisabledByManifest,
    ] {
        assert_eq!(setting(Ok(s)), "blocked");
    }
    assert_eq!(
        setting(Err(windows::core::Error::from_hresult(
            windows::core::HRESULT(0x80070490u32 as i32)
        ))),
        "allowed",
        "no setting recorded before the first toast is the OS default"
    );
    assert_eq!(setting(Ok(NotificationSetting(99))), "unknown");
    assert_eq!(
        setting(Err(windows::core::Error::from_hresult(
            windows::core::HRESULT(0x80004005u32 as i32)
        ))),
        "unknown"
    );
}

// Chain: M14 Windows identity. Risk: registration overwrites another app or saves the wrong identity/target.
#[test]
fn shortcut_identity_readback_and_ownership() -> Result<()> {
    unsafe {
        RoInitialize(RO_INIT_SINGLETHREADED)?;
    }
    let dir = std::env::temp_dir().join(format!("taskq-toast-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("TaskQ.lnk");
    let exe = std::env::current_exe().unwrap();
    shortcut(&path, &exe)?;
    shortcut(&path, &exe)?;
    unsafe {
        let mut key = windows::Win32::Foundation::PROPERTYKEY::default();
        PSGetPropertyKeyFromName(w!("System.AppUserModel.ID"), &mut key)?;
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let file: IPersistFile = link.cast()?;
        file.Load(&HSTRING::from(path.as_os_str()), STGM_READWRITE)?;
        // M15: the launcher opens TaskQ with no arguments, started minimized so no console shows
        let mut args = [0u16; 32];
        link.GetArguments(&mut args)?;
        assert_eq!(args[0], 0);
        assert_eq!(
            link.GetShowCmd()?,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWMINNOACTIVE
        );
        let mut icon_path = [0u16; 1024];
        let mut icon_index = 0;
        link.GetIconLocation(&mut icon_path, &mut icon_index)?;
        let end = icon_path.iter().position(|c| *c == 0).unwrap();
        assert_eq!(
            std::path::PathBuf::from(String::from_utf16_lossy(&icon_path[..end])),
            exe
        );
        assert_eq!(icon_index, 0);
        // The shortcut used by toasts must resolve to an actual icon in this built PE, not a generic fallback.
        assert!(
            windows::Win32::UI::Shell::ExtractIconExW(
                &HSTRING::from(exe.as_os_str()),
                -1,
                None,
                None,
                0
            ) > 0
        );
        let props: IPropertyStore = link.cast()?;
        let mut activator = windows::Win32::Foundation::PROPERTYKEY::default();
        PSGetPropertyKeyFromName(
            w!("System.AppUserModel.ToastActivatorCLSID"),
            &mut activator,
        )?;
        assert_eq!(PropVariantToGUID(&props.GetValue(&activator)?)?, ACTIVATOR);
        props.SetValue(&key, &PROPVARIANT::from("another.app"))?;
        props.Commit()?;
        file.Save(&HSTRING::from(path.as_os_str()), true)?;
    }
    let before = std::fs::read(&path).unwrap();
    assert!(shortcut(&path, &exe).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
    unsafe {
        windows::Win32::System::WinRT::RoUninitialize();
    }
    Ok(())
}

// Chain: M15 launcher ownership (#1758). Risk: a test or temp-install tray points the user's Start menu TaskQ, and
// its taskbar pin, at a folder about to be removed; or a wrong target such a tray left stays broken.
#[test]
fn only_the_default_install_writes_and_repairs_the_start_menu_link() -> Result<()> {
    unsafe {
        RoInitialize(RO_INIT_SINGLETHREADED)?;
    }
    let dir = std::env::temp_dir().join(format!("taskq-launcher-{}", std::process::id()));
    let (home, temp, link) = (dir.join("home"), dir.join("temp"), dir.join("TaskQ.lnk"));
    for install in [&home, &temp] {
        std::fs::create_dir_all(install).unwrap();
        std::fs::write(install.join("taskq.exe"), b"").unwrap();
    }
    assert_eq!(register(&temp, Some(&home), &link), Ok(false));
    assert!(
        !link.exists(),
        "a non-default install never creates the user's Start menu link"
    );
    // a wrong target an older temp-install tray left behind
    shortcut(&link, &temp.join("taskq.exe"))?;
    let before = std::fs::read(&link).unwrap();
    assert_eq!(register(&temp, Some(&home), &link), Ok(false));
    assert_eq!(register(&temp, None, &link), Ok(false));
    assert_eq!(
        std::fs::read(&link).unwrap(),
        before,
        "a non-default install leaves the link untouched"
    );
    assert_eq!(register(&home, Some(&home), &link), Ok(true));
    let target = unsafe {
        let link_file: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link_file
            .cast::<IPersistFile>()?
            .Load(&HSTRING::from(link.as_os_str()), STGM_READ)?;
        let mut path = [0u16; 1024];
        // SLGP_RAWPATH: the stored target, not one the shell resolved
        link_file.GetPath(&mut path, std::ptr::null_mut(), 4)?;
        std::path::PathBuf::from(String::from_utf16_lossy(
            &path[..path.iter().position(|c| *c == 0).unwrap()],
        ))
    };
    // canonical: the shell stores the long form of an 8.3 temp path (CI: RUNNER~1)
    assert_eq!(
        std::fs::canonicalize(target).unwrap(),
        std::fs::canonicalize(home.join("taskq.exe")).unwrap(),
        "the default tray repairs a wrong target on start"
    );
    std::fs::remove_dir_all(dir).unwrap();
    unsafe {
        windows::Win32::System::WinRT::RoUninitialize();
    }
    Ok(())
}

// Chain: M14 Windows toast payload. Risk: owner text changes XML/sound or summary identifiers do not replace.
#[test]
fn native_xml_preserves_text_sound_and_summary_identity() -> Result<()> {
    unsafe {
        RoInitialize(RO_INIT_SINGLETHREADED)?;
    }
    let mut p = Post {
        root: "C:/project".into(),
        n: Some(7),
        title: "<question> & \"owner\"".into(),
        body: "#7 user's task 🦀".into(),
        sound: false,
        id: "{\"root\":\"C:/project\",\"sum\":1}".into(),
    };
    for sound in [false, true] {
        p.sound = sound;
        let doc = xml(&p)?;
        let texts = doc.GetElementsByTagName(&HSTRING::from("text"))?;
        assert_eq!(texts.Item(0)?.InnerText()?.to_string(), p.title);
        assert_eq!(texts.Item(1)?.InnerText()?.to_string(), p.body);
        let t = ToastNotification::CreateToastNotification(&doc)?;
        t.SetTag(&HSTRING::from(tag(&p.id)))?;
        t.SetGroup(&HSTRING::from(tag(&p.root)))?;
        assert_eq!(t.Tag()?.to_string(), tag(&p.id));
        assert_eq!(t.Group()?.to_string(), tag(&p.root));
        assert_eq!(
            doc.GetXml()?.to_string().contains("silent=\"true\""),
            !sound
        );
        assert_eq!(
            doc.DocumentElement()?
                .GetAttribute(&HSTRING::from("launch"))?
                .to_string(),
            p.id
        );
    }
    // Invoke the production COM activator through its class factory: API evidence, not a native user click.
    let (tx, rx) = std::sync::mpsc::channel();
    assert!(
        CLICK
            .set(Mutex::new(Box::new(move |id| {
                tx.send(id).unwrap();
            })))
            .is_ok()
    );
    let factory: IClassFactory = Factory.into();
    let a: INotificationActivationCallback = unsafe { factory.CreateInstance(None)? };
    unsafe {
        a.Activate(w!("org.taskq.tray"), &HSTRING::from(&p.id), &[])?;
    }
    assert_eq!(rx.recv().unwrap(), p.id);
    assert_eq!(tag(&p.id).len(), 16);
    assert_ne!(tag(&p.id), tag("another event"));
    unsafe {
        windows::Win32::System::WinRT::RoUninitialize();
    }
    Ok(())
}
