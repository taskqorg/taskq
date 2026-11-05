    use super::*;
    use std::sync::Mutex;

    /// The active bundle is process-wide, so these tests run one at a time.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// CoreQ keeps its digest private; `verify` names it when the digest is not listed.
    fn digest(bytes: &[u8]) -> String {
        coreq::update::verify(b"", "file", bytes).unwrap_err().split_whitespace().nth(2).unwrap().into()
    }

    struct Fixture {
        _serial: std::sync::MutexGuard<'static, ()>,
        install: PathBuf,
        publication: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
            reset_for_test();
            let base = std::env::temp_dir().join(format!("taskq-ui-bundle-{}", crate::runtime::uuid()));
            let (install, publication) = (base.join("install"), base.join("publication"));
            fs::create_dir_all(&install).unwrap();
            fs::create_dir_all(&publication).unwrap();
            Fixture { _serial: serial, install, publication }
        }

        /// Writes the layout `publish-ui` publishes, with `pages` as its page text.
        fn publish(&self, version: &str, bridge: u64, page: &str) {
            let tag = format!("ui-{version}");
            let dir = self.publication.join(&tag);
            fs::create_dir_all(&dir).unwrap();
            let contents = [("tray.html", page.to_string()), ("workspace.html", page.to_string()), ("operations.json", "[\"bundle.only\"]".to_string())];
            let mut files = serde_json::Map::new();
            let mut sums = String::new();
            for (file, text) in &contents {
                files.insert((*file).into(), json!({"sha256":digest(text.as_bytes()),"size":text.len()}));
                fs::write(dir.join(asset_name(version, file)), text).unwrap();
                sums += &format!("{}  {}\n", digest(text.as_bytes()), asset_name(version, file));
            }
            let manifest = json!({"schema":1,"bundle":version,"bridge":bridge,"files":files}).to_string();
            fs::write(dir.join(asset_name(version, MANIFEST_FILE)), &manifest).unwrap();
            sums += &format!("{}  {}\n", digest(manifest.as_bytes()), asset_name(version, MANIFEST_FILE));
            fs::write(dir.join("SHA256SUMS"), sums).unwrap();
            let channel = self.publication.join(CHANNEL_TAG);
            fs::create_dir_all(&channel).unwrap();
            fs::write(channel.join(pointer_name()), json!({"schema":1,"bundle":version,"bridge":*BRIDGE,"tag":tag}).to_string()).unwrap();
        }

        fn fetch(&self) -> impl Fn(&str, &str, &Path) -> Result<(), String> + '_ {
            |tag, name, to| fs::copy(self.publication.join(tag).join(name), to).map(|_| ()).map_err(|e| format!("network: {e}"))
        }

        fn adopt(&self, store: &mut Store) -> Checked {
            let checked = store.check().run(&self.fetch());
            store.apply(checked.clone());
            checked
        }
    }

    const PAGE: &str = "<!doctype html><html><head><title>bundle</title></head><body></body></html>";
    const NEWER: &str = "9999999998-aaaaaaaaaaaa";
    const NEWEST: &str = "9999999999-bbbbbbbbbbbb";

    // #2365 A5: a fresh offline install opens the embedded copy; a network failure stays out of the errors list.
    #[test]
    fn fresh_offline_install_uses_embedded_copy() {
        let f = Fixture::new();
        let mut store = Store::open(&f.install);
        assert!(active().embedded);
        assert!(active().tray.contains("Content-Security-Policy") && active().workspace.contains("Content-Security-Policy"));
        assert!(matches!(f.adopt(&mut store), Checked::Failed(_)));
        assert!(active().embedded);
        assert!(status()["reason"].is_null(), "the running copy is still good");
        assert!(status()["check_error"].is_string());
    }

    // #2365 A3/A5: a newer compatible bundle is on trial until proven, and survives restart only once committed.
    #[test]
    fn newer_bundle_is_adopted_on_trial_then_committed() {
        let f = Fixture::new();
        f.publish(NEWER, *BRIDGE, PAGE);
        let mut store = Store::open(&f.install);
        assert_eq!(f.adopt(&mut store), Checked::Ready(NEWER.into()));
        assert_eq!(active().version, NEWER);
        assert_eq!(status()["source"], "adopted");
        assert!(active().tray.contains("Content-Security-Policy"), "the binary injects the policy into every source");
        assert!(known_operation("bundle.only"));
        assert!(store.on_trial());
        assert!(Store::open(&f.install).state.current.is_none());
        assert!(active().embedded, "an unproven bundle does not survive a restart");
        let mut store = Store::open(&f.install);
        assert_eq!(f.adopt(&mut store), Checked::Ready(NEWER.into()));
        store.commit();
        assert!(!store.on_trial());
        Store::open(&f.install);
        assert_eq!(active().version, NEWER);
    }

    // #2365 A5: a failed start falls back to the previous good copy, then embedded, and is never retried.
    #[test]
    fn failed_start_falls_back_to_previous_then_embedded_with_reason() {
        let f = Fixture::new();
        f.publish(NEWER, *BRIDGE, PAGE);
        let mut store = Store::open(&f.install);
        f.adopt(&mut store);
        store.commit();
        f.publish(NEWEST, *BRIDGE, PAGE);
        assert_eq!(f.adopt(&mut store), Checked::Ready(NEWEST.into()));
        assert!(store.fail(NEWEST, "page fault"));
        assert_eq!(active().version, NEWER);
        let reason = status()["reason"].as_str().unwrap().to_string();
        assert!(reason.contains(NEWEST) && reason.contains("page fault") && reason.contains(NEWER), "{reason}");
        assert_eq!(f.adopt(&mut store), Checked::Current, "a failed bundle is not retried");
        assert!(store.fail(NEWER, "page fault"));
        assert!(active().embedded);
        assert!(!f.install.join("ui").join(NEWER).exists());
        reset_for_test();
        Store::open(&f.install);
        assert!(status()["reason"].as_str().unwrap().contains("page fault"), "the reason survives a restart");
    }

    // #2365 A5: wrong checksums and an incompatible bridge never become active and leave a visible reason.
    #[test]
    fn refused_publications_stay_out_with_visible_reason() {
        let f = Fixture::new();
        f.publish(NEWER, *BRIDGE, PAGE);
        fs::write(f.publication.join(format!("ui-{NEWER}")).join(asset_name(NEWER, "tray.html")), "<head>changed in transit").unwrap();
        let mut store = Store::open(&f.install);
        assert!(matches!(f.adopt(&mut store), Checked::Refused { .. }));
        assert!(active().embedded);
        assert!(status()["reason"].as_str().unwrap().contains("checksum wrong"));
        assert!(!f.install.join("ui").join(NEWER).exists());

        f.publish(NEWEST, *BRIDGE + 1, PAGE);
        assert!(matches!(f.adopt(&mut store), Checked::Refused { .. }));
        assert!(active().embedded);
        assert!(status()["reason"].as_str().unwrap().contains("incompatible"));
        let entries: Vec<_> = fs::read_dir(f.install.join("ui")).unwrap().flatten().map(|e| e.file_name()).filter(|n| n != "state.json").collect();
        assert!(entries.is_empty(), "scratch and refused files are gone: {entries:?}");
    }

    // #2365 A5: a stored copy damaged on disk is refused at start with a reason, not loaded.
    #[test]
    fn damaged_store_falls_back_at_start() {
        let f = Fixture::new();
        f.publish(NEWER, *BRIDGE, PAGE);
        let mut store = Store::open(&f.install);
        f.adopt(&mut store);
        store.commit();
        fs::write(f.install.join("ui").join(NEWER).join("tray.html"), "<head>tampered").unwrap();
        reset_for_test();
        Store::open(&f.install);
        assert!(active().embedded);
        assert!(status()["reason"].as_str().unwrap().contains("checksum wrong"));
    }

    // R12: a pointer is untrusted text; it cannot name a path outside the store or another bridge.
    #[test]
    fn invalid_pointers_name_no_directory() {
        let f = Fixture::new();
        let channel = f.publication.join(CHANNEL_TAG);
        fs::create_dir_all(&channel).unwrap();
        let store = Store::open(&f.install);
        for pointer in [
            json!({"schema":1,"bundle":"../../evil","bridge":*BRIDGE,"tag":"ui-../../evil"}),
            json!({"schema":1,"bundle":"1..2","bridge":*BRIDGE,"tag":"ui-1..2"}),
            json!({"schema":1,"bundle":NEWER,"bridge":*BRIDGE+1,"tag":format!("ui-{NEWER}")}),
            json!({"schema":1,"bundle":NEWER,"bridge":*BRIDGE,"tag":"other"}),
            json!({"schema":2,"bundle":NEWER,"bridge":*BRIDGE,"tag":format!("ui-{NEWER}")}),
        ] {
            fs::write(channel.join(pointer_name()), pointer.to_string()).unwrap();
            assert!(matches!(store.check().run(&f.fetch()), Checked::Failed(_)), "{pointer}");
        }
    }

    #[test]
    fn version_order_compares_epoch_first() {
        assert!(is_newer("1000000000-aaaaaaaaaaaa", "999999999-bbbbbbbbbbbb"));
        assert!(is_newer("5-b", "5-a"));
        assert!(!is_newer("5-a", "5-a"));
        for bad in ["", "1", "-a", "a-b", "1-", "1-A", "1-a/b", "1-a.b"] {
            assert!(!valid_version(bad), "{bad}");
        }
    }

    // #2365 A6: every source carries the policy; a page with no head could place content ahead of it.
    #[test]
    fn page_policy_refuses_remote_and_frame_sources() {
        let hardened = harden(PAGE).unwrap();
        assert!(hardened.find("Content-Security-Policy").unwrap() < hardened.find("<title>").unwrap());
        assert!(harden("<html><body></body></html>").is_err());
        for needed in ["default-src 'none'", "connect-src 'none'", "frame-src 'none'", "base-uri 'none'"] {
            assert!(CSP.contains(needed), "{needed}");
        }
        assert!(!CSP.contains("http"), "the policy names no remote source");
    }
