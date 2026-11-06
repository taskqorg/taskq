    use super::*;

    /// `restore` refuses a worker, which is how a worker's `cargo test` left this fixture behind on every run (#2417).
    fn operator_restore(install: &Path) {
        if std::env::var_os("TASKQ_ATTEMPT").is_none() { return restore(install).unwrap(); }
        assert_eq!(restore(install).unwrap_err(), "Windows security restore requires an unrestricted operator");
        restore_journal(install, None).unwrap();
    }

    // #2417 defect: a fixture removal restores from a journal in a temp folder, and such a file can be forged. Protection:
    // a journal naming a path outside its folder changes nothing, there or anywhere.
    #[test]
    fn fixture_removal_refuses_a_journal_outside_its_folder() {
        let stamp = crate::runtime::uuid();
        let name = |tag: &str| std::env::temp_dir().join(format!("taskq-preparation-forged-{tag}-{stamp}"));
        let (own, other) = (name("own"), name("other"));
        let _guards = (prepared_fixture(&own), prepared_fixture(&other));
        let journal = "install/windows-native/security-preparation.json";
        let saved = fs::read(own.join(journal)).unwrap();
        fs::copy(other.join(journal), own.join(journal)).unwrap();
        assert!(remove_fixture(&own).unwrap_err().contains("outside its preparation folder"));
        assert_eq!(fs::remove_file(other.join("home/state_5.sqlite-wal")).unwrap_err().raw_os_error(), Some(5), "the other folder keeps its denials");
        fs::write(own.join(journal), saved).unwrap();
    }

    #[test]
    #[ignore = "native Windows security journal fixture"]
    fn native_preparation_tolerates_sdk_temp_cleanup() {
        let base = std::env::temp_dir().join(format!("taskq-preparation-temp-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let _fixture = Fixture(base.clone());
        let root = base.join("workspace"); let install = base.join("install"); let home = base.join("home");
        let state = install.join("windows-native/state"); let temp = base.join("temp");
        let state_temp = state.join("TMP"); let generated = state_temp.join("arg0/codex-arg0-fixture");
        for path in [root.join(".codex"), generated.clone(), home.clone(), temp.clone()] { fs::create_dir_all(path).unwrap(); }
        fs::write(generated.join(".lock"), "fixture lock").unwrap();
        fs::write(generated.join("apply_patch.bat"), "fixture patch helper").unwrap();
        let config = home.join("config.toml"); let control = state.join("control.json");
        fs::write(&config, "# invented fixture configuration\n").unwrap();
        fs::write(&control, "{}").unwrap();
        let before = [Snapshot::capture(&config).unwrap(), Snapshot::capture(&control).unwrap(), Snapshot::capture(&state_temp).unwrap()];
        let preparation = Preparation::begin(&root, &install, &home, &state, &temp, &[]).unwrap();
        let boundary = super::super::checked(&state_temp).unwrap();
        assert!(preparation.journal.records.iter().any(|r| r.original.path == boundary));
        assert!(!preparation.journal.records.iter().any(|r| r.original.path != boundary && r.original.path.starts_with(&boundary)));
        for original in &before {
            assert!(preparation.journal.records.iter().any(|r| r.original == *original));
        }
        // The SDK legitimately removes previous generated helpers during preflight.
        fs::remove_dir_all(state_temp.join("arg0")).unwrap();
        preparation.apply().unwrap();
        for path in [&state, &state_temp, &config, &control] {
            assert!(Snapshot::capture(path).unwrap().descriptor.contains(";;;ME)"));
        }
        let fresh = state_temp.join("new-helper.bat"); fs::write(&fresh, "new SDK helper").unwrap();
        assert!(Snapshot::capture(&fresh).unwrap().descriptor.contains(";;;ME)"));
        fs::write(&config, "# owner changes survive rollback\n").unwrap();
        restore(&install).unwrap();
        for original in before { assert_eq!(Snapshot::capture(&original.path).unwrap(), original); }
        assert_eq!(fs::read_to_string(&config).unwrap(), "# owner changes survive rollback\n");
        assert_eq!(fs::read_to_string(&fresh).unwrap(), "new SDK helper");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn native_preparation_and_restore_preserve_controls_and_data() {
        let base = std::env::temp_dir().join(format!("taskq-preparation-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let _fixture = Fixture(base.clone());
        let root = base.join("workspace"); let install = base.join("install"); let home = base.join("home");
        let state = install.join("windows-native/state"); let temp = base.join("temp");
        for path in [root.join(".CODEX"), state.clone(), home.clone(), temp.clone()] { fs::create_dir_all(path).unwrap(); }
        let overlapping = install.join("workspace"); fs::create_dir(&overlapping).unwrap();
        assert!(Preparation::begin(&overlapping, &install, &home, &state, &temp, &[]).is_err());
        assert!(!install.join("windows-native/security-preparation.json").exists(), "overlap refusal precedes effects");
        fs::write(home.join("config.TOML"), "# invented fixture configuration\n").unwrap();
        fs::write(root.join(".codex/control.toml"), "# invented fixture control\n").unwrap();
        fs::write(root.join("data.txt"), "before").unwrap();
        let before = Snapshot::capture(&root).unwrap();
        let home_before = Snapshot::capture(&home).unwrap();
        Preparation::begin(&root, &install, &home, &state, &temp, &[]).unwrap().apply().unwrap();
        assert_eq!(Snapshot::capture(&root).unwrap(), before.low().unwrap());
        assert!(Snapshot::capture(&root.join(".codex/control.toml")).unwrap().descriptor.contains(";;;ME)"));
        assert!(Snapshot::capture(&home.join("config.toml")).unwrap().descriptor.contains(";;;ME)"));
        fs::write(root.join("data.txt"), "owner data survives rollback").unwrap();
        fs::write(home.join("owner-config.tmp"), "owner rewrite").unwrap();
        fs::rename(home.join("owner-config.tmp"), home.join("config.toml")).unwrap();
        let wal = home.join("state_5.sqlite-wal");
        assert_eq!(fs::remove_file(&wal).unwrap_err().raw_os_error(), Some(5));
        // Replacing an owner control is a conflict, not permission to restore over its new identity.
        operator_restore(&install);
        assert_eq!(Snapshot::capture(&root).unwrap(), before);
        assert_eq!(Snapshot::capture(&home).unwrap(), home_before);
        assert_eq!(fs::read_to_string(root.join("data.txt")).unwrap(), "owner data survives rollback");
        assert_eq!(fs::read_to_string(home.join("config.toml")).unwrap(), "owner rewrite");
        fs::remove_file(&wal).unwrap();
        fs::remove_dir_all(&base).unwrap();
    }
    /// Removes a fixture on drop, so a test that panics between `apply` and its own cleanup leaves no denials in temp.
    pub(crate) struct Fixture(PathBuf);
    impl Drop for Fixture { fn drop(&mut self) { let _ = remove_fixture(&self.0); } }

    pub(crate) fn prepared_fixture(base: &Path) -> Fixture {
        let fixture = Fixture(base.to_owned());
        let (root, install, home, temp) = (base.join("workspace"), base.join("install"), base.join("home"), base.join("temp"));
        let state = install.join("windows-native/state");
        for path in [root.join(".codex"), state.clone(), home.clone(), temp.clone()] { fs::create_dir_all(path).unwrap(); }
        fs::write(root.join("data.txt"), "fixture data").unwrap();
        fs::write(home.join("config.toml"), "# invented fixture configuration\n").unwrap();
        Preparation::begin(&root, &install, &home, &state, &temp, &[]).unwrap().apply().unwrap();
        fixture
    }
