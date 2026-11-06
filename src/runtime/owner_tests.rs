    use super::*;
    use serde_json::json;

    // A5: a warm directory memo avoids entry scans once its stamps settle, but tree changes still reveal ambiguity and lost paths refuse.
    #[test]
    fn codex_path_memo_revalidates_tree_and_never_substitutes_lost_source() {
        let root=std::env::temp_dir().join(format!("taskq-source-paths-{}",super::super::uuid()));
        let dir=root.join("sessions/2026/11/05");fs::create_dir_all(&dir).unwrap();let session=super::super::uuid();let file=format!("rollout-invented-{session}.jsonl");let path=dir.join(&file);fs::write(&path,"invented").unwrap();
        std::thread::sleep(STAMP_SETTLE+std::time::Duration::from_millis(100));
        assert_eq!(codex_transcript_in(&root,&session).unwrap(),path);assert_eq!(codex_transcript_in(&root,&session).unwrap(),path);
        assert_eq!(crate::profile::LAST_SOURCE_WORK.with(|c|c.borrow().get("path_cache_hits").copied()),Some(1));
        let duplicate=root.join("sessions/2026/11/06");fs::create_dir_all(&duplicate).unwrap();fs::write(duplicate.join(&file),"invented").unwrap();
        assert!(codex_transcript_in(&root,&session).unwrap_err().contains("multiple"));
        fs::remove_file(&path).unwrap();assert!(codex_transcript_in(&root,&session).unwrap_err().contains("lost"));
        // Revalidating a different session must not erase this session's accepted-path fence.
        let other=super::super::uuid();fs::write(duplicate.join(format!("rollout-invented-{other}.jsonl")),"invented").unwrap();
        codex_transcript_in(&root,&other).unwrap();
        assert!(codex_transcript_in(&root,&session).unwrap_err().contains("lost"));
        fs::remove_dir_all(root).unwrap();
    }

    /// Puts a directory back on an exact stamp, as a change within one filesystem clock tick leaves it.
    fn restore_stamp(dir:&Path,at:std::time::SystemTime){
        #[cfg(windows)] let handle={use std::os::windows::fs::OpenOptionsExt;fs::OpenOptions::new().access_mode(0x100).custom_flags(0x0200_0000).open(dir).unwrap()};// FILE_WRITE_ATTRIBUTES, FILE_FLAG_BACKUP_SEMANTICS
        #[cfg(not(windows))] let handle=fs::File::open(dir).unwrap();
        handle.set_modified(at).unwrap();assert_eq!(fs::metadata(dir).unwrap().modified().unwrap(),at,"fixture must keep the stamp unchanged");
    }

    // #2415 R12: a tree change in the clock tick of a just-recorded stamp keeps that stamp, so a fresh stamp proves nothing.
    // Forcing the collision makes the Windows race deterministic: unfixed, the memo returned Ok for the duplicate.
    #[test]
    fn colliding_directory_stamps_never_hide_a_duplicate_or_replace_a_source() {
        let root=std::env::temp_dir().join(format!("taskq-source-collision-{}",super::super::uuid()));
        let month=root.join("sessions/2026/11");let dir=month.join("05");fs::create_dir_all(&dir).unwrap();
        let session=super::super::uuid();let file=format!("rollout-invented-{session}.jsonl");let path=dir.join(&file);fs::write(&path,"invented").unwrap();
        assert_eq!(codex_transcript_in(&root,&session).unwrap(),path);
        assert_eq!(codex_transcript_in(&root,&session).unwrap(),path);
        assert_eq!(crate::profile::LAST_SOURCE_WORK.with(|c|c.borrow().get("path_cache_hits").copied()),None,"a fresh stamp is no scan proof");
        let stamp=fs::metadata(&month).unwrap().modified().unwrap();
        let duplicate=month.join("06");fs::create_dir_all(&duplicate).unwrap();fs::write(duplicate.join(&file),"invented").unwrap();restore_stamp(&month,stamp);
        assert!(codex_transcript_in(&root,&session).unwrap_err().contains("multiple"),"duplicate stays unknown under colliding stamps");
        // Removing the duplicate keeps the accepted source; a replacement after losing it never substitutes.
        fs::remove_dir_all(&duplicate).unwrap();restore_stamp(&month,stamp);
        assert_eq!(codex_transcript_in(&root,&session).unwrap(),path);
        fs::remove_file(&path).unwrap();let moved=month.join("07");fs::create_dir_all(&moved).unwrap();fs::write(moved.join(&file),"invented").unwrap();restore_stamp(&month,stamp);
        assert!(codex_transcript_in(&root,&session).unwrap_err().contains("lost"),"a replaced source stays refused");
        assert!(codex_transcript_in(&root,&super::super::uuid()).unwrap_err().contains("unavailable"),"a missing source stays unknown");
        fs::remove_dir_all(root).unwrap();
    }

    // R12 / #2022: Windows owner reads must resolve the same native homes as worker recovery, including overrides.
    #[test]
    fn owner_transcripts_use_native_homes() {
        let root = std::env::temp_dir().join(format!("taskq-owner-homes-{}", super::super::uuid()));
        fs::create_dir_all(&root).unwrap();
        for overrides in [false, true] {
            let mut c = std::process::Command::new(std::env::current_exe().unwrap());
            c.args(["--exact", "runtime::owner::tests::owner_transcript_home_fixture", "--ignored", "--nocapture"])
                .env("TASKQ_TRANSCRIPT_FIXTURE", &root).env_remove("CLAUDE_CONFIG_DIR").env_remove("CODEX_HOME")
                .env_remove("HOME").env("USERPROFILE", &root);
            if !cfg!(windows) { c.env("HOME", &root); }
            if overrides { c.env("CLAUDE_CONFIG_DIR", root.join("override-claude")).env("CODEX_HOME", root.join("override-codex")); }
            let out = coreq::process::output("owner.transcript-homes", &mut c, std::time::Duration::from_secs(30)).unwrap();
            assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "isolated home-resolution subprocess"]
    fn owner_transcript_home_fixture() {
        let root = PathBuf::from(std::env::var_os("TASKQ_TRANSCRIPT_FIXTURE").expect("owned fixture root"));
        let session = "11111111-2222-4333-8444-555555555555";
        for runtime in ["claude", "codex"] {
            let home = if runtime == "claude" { super::super::claude_home() } else { super::super::codex_home() }.unwrap();
            assert!(home.starts_with(&root));
            let relative = if runtime == "claude" { format!("projects/invented/{session}.jsonl") }
                else { format!("sessions/2026/10/31/rollout-invented-{session}.jsonl") };
            let path = home.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "invented transcript\n").unwrap();
            assert_eq!(transcript(runtime, session).unwrap(), path);
            assert!(transcript(runtime, "../../escape").is_err());
            assert!(transcript(runtime, "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee").is_err());
            let duplicate = home.join(if runtime == "claude" { format!("projects/duplicate/{session}.jsonl") }
                else { format!("sessions/2026/10/30/rollout-duplicate-{session}.jsonl") });
            fs::create_dir_all(duplicate.parent().unwrap()).unwrap();
            fs::write(&duplicate, "invented duplicate\n").unwrap();
            assert!(transcript(runtime, session).is_err(), "ambiguous native identity must stay unknown");
            assert_eq!(fs::read_to_string(&path).unwrap(), "invented transcript\n");
        }
    }

    // #1749: the idle job TaskQ launched must not refuse the next Send; any other open writer still does (D3 one writer).
    #[test]
    fn only_the_idle_launched_job_is_released() {
        let r: Request = serde_json::from_value(json!({"schema": 1, "id": "c1", "created": 0, "host": "mac", "platform": "macos", "client": "tray", "runtime": "claude",
            "targets": [], "brief": "", "state": "launched", "job": "aa11", "session": "s1"})).unwrap();
        // Real `claude agents --json` shape: an idle launched job still reports `state` `working` (#1762).
        let row = |id: &str, kind: &str, status: &str| json!({"id": id, "kind": kind, "sessionId": "s1", "status": status, "state": "working"});
        assert_eq!(holds(&[], &r), Hold::Free);
        assert_eq!(holds(&[json!({"sessionId": "s2", "kind": "interactive"})], &r), Hold::Free);
        assert_eq!(holds(&[row("aa11", "background", "idle")], &r), Hold::Own);
        assert_eq!(holds(&[row("aa11", "background", "busy")], &r), Hold::Other, "a working job is a writer");
        assert_eq!(holds(&[row("bb22", "background", "idle")], &r), Hold::Other, "another job holds");
        assert_eq!(holds(&[row("aa11", "background", "idle"), json!({"sessionId": "s1", "kind": "interactive", "status": "idle"})], &r), Hold::Other, "an open terminal holds");
    }
