    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("taskq-doctor-{tag}-{}", crate::runtime::uuid()));
        fs::create_dir_all(&d).unwrap();
        d
    }
    fn file(p: &Path, bytes: usize, at: SystemTime) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![b'x'; bytes]).unwrap();
        fs::File::options().write(true).open(p).unwrap().set_modified(at).unwrap();
    }
    fn names(dir: &Path) -> Vec<String> {
        let mut n: Vec<String> = fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        n.sort();
        n
    }

    // #2048 defect: only a release update swept old releases, so a dev-channel host kept 299 (4.3 GB). The doctor keeps
    // releases in use plus the two newest others and removes the rest, with their bytes counted.
    #[test]
    fn doctor_keeps_releases_in_use_and_two_previous() {
        let base = scratch("releases");
        let install = base.join("install");
        let now = SystemTime::now() + Duration::from_secs(2 * 86_400);
        let day = |n: u64| SystemTime::now() - Duration::from_secs(n * 86_400);
        for (i, v) in ["4.0.1", "4.0.2", "4.0.3", "4.0.4", "4.0.5", "4.0.6"].iter().enumerate() {
            file(&install.join("releases").join(v).join("taskq"), 100, day(10 - i as u64));
        }
        file(&install.join("releases/4.0.7.tmp/taskq"), 10, day(30));
        file(&install.join("releases/4.0.8.tmp/taskq"), 10, now);
        fs::write(install.join("current"), "4.0.2").unwrap();
        fs::write(install.join("previous"), "4.0.1").unwrap();
        fs::write(install.join("projects"), "").unwrap();
        let o = pass(&install, &base.join("none"), now, &Native::None, false);
        let on_disk = names(&install.join("releases"));
        for v in ["4.0.1", "4.0.2", "4.0.5", "4.0.6", "4.0.8.tmp"] { assert!(on_disk.contains(&v.to_string()), "{v} kept: {on_disk:?}"); }
        for v in ["4.0.3", "4.0.4", "4.0.7.tmp"] { assert!(!on_disk.contains(&v.to_string()), "{v} removed: {on_disk:?}"); }
        assert_eq!((o.releases, o.freed, o.kept, o.failed, o.unknown), (3, 210, 4, 0, 0));
        let _ = fs::remove_dir_all(&base);
    }

    // #2417 defect: a test that died before restoring left `taskq-preparation-*` with its denials. `remove_dir_all` then
    // stopped at the first WAL/SHM sidecar with os error 5, on every pass, so the folders piled up in the OS temp dir.
    // Protection: the doctor restores such a fixture from its own journal and removes it, including one a failed pass
    // already emptied in part. Any other folder gets no restore: its failure stays reported.
    #[cfg(windows)]
    #[test]
    fn doctor_removes_a_preparation_folder_its_denials_block() {
        let temp = scratch("preparation");
        let fixture = temp.join("taskq-preparation-1-2");
        let _fixture = crate::windows_sandbox::prepared_fixture(&fixture);
        let other = crate::windows_sandbox::prepared_fixture(&temp.join("taskq-other"));
        assert_eq!(fs::remove_dir_all(&fixture).unwrap_err().raw_os_error(), Some(5), "the denials block a plain removal");
        assert!(fixture.join("install/windows-native/security-preparation.json").exists(), "the failed removal leaves the journal");
        let later = SystemTime::now() + Duration::from_secs(2 * 86_400);
        let o = pass(&temp.join("install"), &temp, later, &Native::None, false);
        assert_eq!((o.temp, o.failed), (1, 1), "{:?}", o.lines);
        assert_eq!(names(&temp), vec!["taskq-other".to_string()]);
        drop(other);
        fs::remove_dir_all(&temp).unwrap();
    }

    // #2060 defect: the doctor ran only hourly and printed no item, so nobody could preview or force a pass. Kept
    // protection: `taskq doctor --dry-run` removes nothing, and a running pass (the service's) blocks a second one.
    #[test]
    fn doctor_dry_run_lists_items_and_removes_nothing() {
        let base = scratch("dry");
        let install = base.join("install");
        let old = SystemTime::now() - Duration::from_secs(10 * 86_400);
        for v in ["4.0.1", "4.0.2", "4.0.3", "4.0.4"] { file(&install.join("releases").join(v).join("taskq"), 100, old); }
        file(&base.join("temp/taskq-candidate-1/x"), 5, old);
        fs::write(install.join("current"), "4.0.4").unwrap();
        fs::write(install.join("projects"), "").unwrap();
        let o = pass(&install, &base.join("temp"), SystemTime::now(), &Native::None, true);
        assert_eq!(names(&install.join("releases")).len(), 4);
        assert_eq!(names(&base.join("temp")), ["taskq-candidate-1"]);
        let lines = o.cli_lines();
        assert_eq!(lines.iter().filter(|l| l.starts_with("would remove release 4.0.1")).count(), 1, "{lines:?}");
        assert!(lines.contains(&"would remove temp folder taskq-candidate-1".into()), "{lines:?}");
        assert_eq!(lines.last().unwrap(), "doctor: would remove 1 old releases, 1 temp folders, 0 qualification objects; would free 0 MiB; kept 3 releases");
        let o = pass(&install, &base.join("temp"), SystemTime::now(), &Native::None, false);
        assert_eq!(o.cli_lines().last().unwrap(), "doctor: removed 1 old releases, 1 temp folders, 0 qualification objects; freed 0 MiB; kept 3 releases");
        assert_eq!(pass(&install, &base.join("temp"), SystemTime::now(), &Native::None, false).cli_lines(), ["doctor: nothing to remove; kept 3 releases"]);
        let held = lock(&install).unwrap();
        assert_eq!(cli(&install, true), ["doctor: a pass is running"]);
        drop(held);
        let _ = fs::remove_dir_all(&base);
    }

    // Kept protection: an unreadable registry leaves release ownership unknown, so no release is removed.
    #[test]
    fn doctor_keeps_every_release_when_ownership_is_unknown() {
        let base = scratch("unknown");
        let install = base.join("install");
        for v in ["4.0.1", "4.0.2", "4.0.3", "4.0.4"] { file(&install.join("releases").join(v).join("taskq"), 1, SystemTime::UNIX_EPOCH + Duration::from_secs(86_400)); }
        fs::write(install.join("current"), "4.0.4").unwrap();
        fs::create_dir_all(install.join("projects")).unwrap();
        let o = pass(&install, &base.join("none"), SystemTime::now(), &Native::None, false);
        assert_eq!(names(&install.join("releases")).len(), 4);
        assert_eq!((o.releases, o.unknown, o.kept), (0, 1, 4));
        assert!(o.line().unwrap().contains("ownership unknown"));
        let _ = fs::remove_dir_all(&base);
    }

    // #2048 defect: qualification and test scratch stayed in the OS temp dir for good. Kept protection: live worker
    // private temp, foreign folders, files and anything touched within a day stay.
    #[test]
    fn doctor_removes_stale_temp_leftovers_only() {
        let base = scratch("temp");
        let temp = base.join("temp");
        let now = SystemTime::now() + Duration::from_secs(2 * 86_400);
        let old = SystemTime::now();
        file(&temp.join("taskq-candidate-a422e70/evidence/run.log"), 40, old);
        file(&temp.join("coreq-host-installer-qualification-1/x"), 2, old);
        file(&temp.join("taskq-windows/abc/live"), 5, old);
        file(&temp.join("taskq-setup-1/old"), 3, old);
        file(&temp.join("taskq-setup-1/fresh"), 3, now);
        file(&temp.join("other-tool/x"), 1, old);
        file(&temp.join("taskq-tray.lock"), 1, old);
        let o = pass(&base.join("install"), &temp, now, &Native::None, false);
        assert_eq!(names(&temp), ["other-tool", "taskq-setup-1", "taskq-tray.lock", "taskq-windows"]);
        assert_eq!((o.temp, o.freed, o.failed), (2, 42, 0));
        assert_eq!(o.counts()["removed"], 2);
        assert_eq!(o.counts()["freed_bytes"], 42);
        let _ = fs::remove_dir_all(&base);
    }

    // #2048 defect: Scheduled Tasks and user units of qualification runs stayed registered. Kept protection: the host's
    // own unit, running entries, fresh entries and never-run entries stay.
    #[test]
    fn doctor_selects_idle_stale_qualification_objects() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10 * 86_400);
        let old = 8 * 86_400;
        let tasks = windows_tasks(&format!("CodexQualification-a422e70\t3\t{old}\nQualification-F90-TaskQ-Runtime\t1\t{old}\nTaskQ\t4\t{old}\n\
            Claude-Canary-run\t4\t{old}\nTaskQ-Qualification-new\t3\t{}\nTaskQ-Qualification-never\t3\t0\nOneDrive Qualification\t3\t{old}\n", 10 * 86_400 - 60));
        assert_eq!(removable(&tasks, now, None), ["CodexQualification-a422e70", "Qualification-F90-TaskQ-Runtime"]);
        let units = linux_units(&format!("Id=taskq-stop-1.service\nStateChangeTimestampMonotonic={}\n\nId=taskq-qualification-x.service\n\
            StateChangeTimestampMonotonic={}\n\nId=other.service\nStateChangeTimestampMonotonic=1000000\n", 1_000_000, (5 * 86_400 - 60) as u64 * 1_000_000),
            5.0 * 86_400.0, now);
        assert_eq!(removable(&units, now, Some("taskq-")), ["taskq-stop-1.service"]);
        assert_eq!(mac_jobs("-\t0\tcom.taskq.qualification.f90\n123\t0\tcom.taskq.canary.live\n-\t0\tcom.taskq.service\n-\t0\tcom.apple.x\n"),
            ["com.taskq.qualification.f90"]);
        assert!(!qualification_object("TaskQ") && !qualification_object("taskq.service"));
    }
