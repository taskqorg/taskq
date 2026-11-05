    use super::*;

    fn script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let (p, text) = if cfg!(windows) { (dir.join(format!("{name}.cmd")), format!("@echo off\r\n{}\r\n", body.replace('\n', "\r\n"))) }
            else { (dir.join(name), format!("#!/bin/sh\n{body}\n")) };
        if cfg!(windows) { std::fs::write(&p, text).unwrap(); return p; }
        // A write fd held here leaks into a child another test thread forks, so exec fails with ETXTBSY (os error 26);
        // `cp` writes the executable from its own process instead.
        let src = dir.join(format!("{name}.src"));
        std::fs::write(&src, text).unwrap();
        #[cfg(unix)] std::fs::set_permissions(&src, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        assert!(Command::new("cp").arg(&src).arg(&p).status().unwrap().success());
        p
    }

    #[test]
    fn version_compare_reads_numeric_prefix() {
        assert!(below("2.16.9", "2.17.0") && below("2.5.0", "2.5.1") && below("7.9", "7.55.0"));
        assert!(!below("2.46.0.windows.1", "2.17.0") && !below("2.17", "2.17.0") && !below("0.162.1", "0.159.3"));
    }

    // Chain: #2329 os error 32 on fixture cleanup. Risk: a timed-out tool's surviving descendant keeps the caller's
    // directory in use; the probe process itself runs in the tool's directory.
    #[test]
    fn version_probe_never_runs_in_the_callers_directory() {
        let dir = std::env::temp_dir().join(format!("taskq-tools-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tool = script(&dir, "tool", if cfg!(windows) { "cd > probe-cwd
echo git version 2.46.0" } else { "pwd > probe-cwd
echo git version 2.46.0" });
        assert_eq!(version("git", &tool), Ok("2.46.0".into()));
        let cwd = std::fs::read_to_string(dir.join("probe-cwd")).unwrap();
        assert_eq!(Path::new(cwd.trim()).canonicalize().unwrap(), dir.canonicalize().unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }

    // Chain: #1856 host requirements (specq#98, pilotq#127). Risk: an old or missing tool passes selftest or reads as
    // an empty result; each failure names tool, found version, minimum and fix.
    #[test]
    fn old_missing_and_failing_tools_fail_with_their_fix() {
        let dir = std::env::temp_dir().join(format!("taskq-tools-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let git = &TOOLS[1];
        let ok = check(git, &script(&dir, "new", "echo git version 2.46.0.windows.1"), true);
        assert_eq!(ok, Ok("git: 2.46.0.windows.1 (minimum 2.17.0) PASS".into()));
        let old = check(git, &script(&dir, "old", "echo git version 2.16.2"), true).unwrap_err();
        assert!(old.starts_with("git: found 2.16.2; minimum 2.17.0 for worktrees") && old.contains("fix: install or update Git"), "{old}");
        let broken = check(git, &script(&dir, "broken", "echo fatal: cannot run 1>&2\nexit 3"), true).unwrap_err();
        assert!(broken.contains("`git --version` failed") && broken.contains("fatal: cannot run") && broken.contains("fix:"), "{broken}");
        let silent = check(git, &script(&dir, "silent", "exit 0"), true).unwrap_err();
        assert!(silent.contains("printed no version: no output"), "{silent}");
        let missing = Path::new("git-not-installed");
        assert_eq!(check(git, missing, true).unwrap_err(), format!("git: not installed; minimum 2.17.0 for {}; fix: {}", git.needs, git.fix));
        assert_eq!(check(&TOOLS[2], missing, false), Ok("gh: not installed (optional)".into()));
        // an installed optional tool below its minimum still fails: a worker may run it
        assert!(check(&TOOLS[2], &script(&dir, "oldgh", "echo gh version 2.4.0 (2022-01-01)"), false).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Chain: #1856 A4. Risk: a failed tool call (nonzero exit, missing binary) reads as empty output; the error keeps
    // the command, exit status and stderr.
    #[test]
    fn failed_tool_calls_keep_command_and_error() {
        let dir = std::env::temp_dir().join(format!("taskq-tools-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fail = script(&dir, "fail", "echo fatal: no remote 1>&2\nexit 2");
        let e = run("test.tool", Command::new(&fail).arg("ls-remote"), Duration::from_secs(10)).unwrap_err();
        assert!(e.contains("fail") && e.contains(" ls-remote` failed: ") && e.contains("2") && e.contains("fatal: no remote"), "{e}");
        let quiet = script(&dir, "quiet", "exit 1");
        assert!(run("test.tool", &mut Command::new(&quiet), Duration::from_secs(10)).unwrap_err().ends_with("no error output"));
        let e = run("test.tool", Command::new(dir.join("not-installed")).arg("--version"), Duration::from_secs(10)).unwrap_err();
        assert!(e.starts_with("`not-installed --version` failed: process spawn"), "{e}");
        assert_eq!(run("test.tool", &mut Command::new(script(&dir, "ok", "echo out")), Duration::from_secs(10)).unwrap().trim_ascii(), b"out");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn requirements_follow_board_and_default_runtime() {
        let req = |tool: &str, github, rt| needs(github, rt).into_iter().find(|n| n.0.tool == tool).map(|n| n.1);
        assert_eq!((req("curl", None, None), req("git", Some(false), None), req("git", None, None)), (Some(true), Some(true), Some(false)));
        assert_eq!((req("gh", Some(true), None), req("gh", None, None), req("gh", Some(false), None)), (Some(true), Some(false), None));
        assert_eq!((req("claude", None, Some("claude")), req("codex", None, Some("claude")), req("claude", None, None)), (Some(true), Some(false), Some(false)));
        assert_eq!((req("codex", None, Some("codex")), req("claude", None, Some("codex"))), (Some(true), Some(false)));
        let rows: Vec<_> = needs(None, None).into_iter().map(|n| (n.0.tool, n.0.min)).collect();
        assert_eq!(&rows[2..], [("gh", "2.5.1"), ("claude", "2.1.296"), ("codex", "0.159.3"), ("agy", "")], "one row per CLI binary");
    }
