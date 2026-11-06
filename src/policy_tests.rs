use super::*;

fn host<'a>(os: &'a str, install: &'a Path, root: &'a Path) -> Host<'a> {
    Host {
        os,
        install,
        root,
        home: Path::new("/Users/u"),
    }
}

fn g(rt: &str, mode: &str, project: Option<&str>) -> Grant {
    Grant {
        runtime: rt.into(),
        mode: mode.into(),
        project: project.map(String::from),
        by: "op".into(),
        at: 1,
    }
}

// Chain: R9 / § 8 Access resolution. Risk: legacy config or a missing grant becomes a grant, or unknown
// enforcement launches.
#[test]
fn grants_resolve_and_unknown_enforcement_refuses() {
    let (i, r) = (Path::new("/i"), Path::new("/p"));
    let ws = [
        "-s".to_string(),
        "workspace-write".into(),
        "--add-dir".into(),
        "/p/.git".into(),
    ];
    let none = Ok(Policy::default());
    // legacy: flags unchanged, never a grant; macOS closes the store, other OS refuse
    let e = resolve(
        &none,
        &host("macos", i, r),
        "claude",
        "bypassPermissions",
        &[],
    );
    assert_eq!(
        (e.mode.as_str(), e.refused.as_ref(), e.requested.as_str()),
        ("legacy", None, "--permission-mode bypassPermissions")
    );
    assert!(
        e.profile
            .unwrap()
            .starts_with("(version 1)\n(allow default)\n(deny file-write* (subpath \"/i\"))\n")
    );
    assert!(
        resolve(
            &none,
            &host("macos", i, r),
            "codex",
            "",
            &["--full-auto".into(), "-m".into(), "gpt-5".into()]
        )
        .refused
        .is_none()
    );
    let e = resolve(&none, &host("macos", i, r), "codex", "", &ws[..2]);
    assert_eq!(
        (e.profile.as_ref(), e.codex.as_ref(), e.enforcement.as_str()),
        (
            None,
            None,
            "codex sandbox workspace-write, policy store not writable"
        )
    );
    if cfg!(unix) {
        assert!(
            resolve(&none, &host("macos", i, r), "codex", "", &ws)
                .refused
                .is_none(),
            "an absolute added dir"
        );
    }
    let up = [
        "-s".to_string(),
        "workspace-write".into(),
        "--add-dir".into(),
        "/".into(),
    ];
    for w in [
        &up[..],
        &[
            "-s".into(),
            "workspace-write".into(),
            "--add-dir".into(),
            "..".into(),
        ],
        &[
            "-s".into(),
            "workspace-write".into(),
            "-c".into(),
            "x=1".into(),
        ],
        &["--full-auto".into(), "--yolo".into()],
        &[
            "-s".into(),
            "workspace-write".into(),
            "-C".into(),
            "/".into(),
        ],
        &[
            "-s".into(),
            "workspace-write".into(),
            "--sandbox=danger-full-access".into(),
        ],
    ] {
        assert!(
            resolve(&none, &host("macos", i, r), "codex", "", w)
                .refused
                .is_some(),
            "{w:?}"
        );
    }
    assert!(
        resolve(
            &none,
            &host("macos", Path::new("/p/i"), r),
            "codex",
            "",
            &ws[..2]
        )
        .refused
        .is_some()
    );
    assert!(
        resolve(&none, &host("windows", i, r), "codex", "", &ws)
            .refused
            .unwrap()
            .contains("no operator grant")
    );
    assert!(
        resolve(&none, &host("macos", i, r), "codex-app", "", &[])
            .refused
            .is_some()
    );
    assert!(
        resolve(
            &Err("x".into()),
            &host("macos", i, r),
            "claude",
            "dontAsk",
            &[]
        )
        .refused
        .unwrap()
        .contains("unreadable")
    );
    // grants: a project grant replaces the host default; full is uncontained everywhere; workspace only where enforced
    let p = Ok(Policy {
        revision: 4,
        grants: vec![
            g("claude", "full", None),
            g("claude", "workspace", Some("/p")),
            g("codex", "restricted", None),
        ],
    });
    let e = resolve(&p, &host("macos", i, r), "claude", "dontAsk", &[]);
    assert_eq!(
        (e.mode.as_str(), e.grant.as_str()),
        ("workspace", "workspace (project) rev 4 by op")
    );
    assert!(e.profile.unwrap().contains("(subpath \"/p\")"));
    let e = resolve(
        &p,
        &host("macos", i, Path::new("/q")),
        "claude",
        "dontAsk",
        &[],
    );
    assert_eq!(
        (
            e.mode.as_str(),
            e.profile.as_ref(),
            e.refused.as_ref(),
            e.claude.as_deref()
        ),
        ("full", None, None, Some("bypassPermissions"))
    );
    assert_eq!(
        resolve(
            &p,
            &host("windows", i, Path::new("/q")),
            "claude",
            "dontAsk",
            &[]
        )
        .refused,
        None
    );
    // Full turns the runtime sandbox off on start and resume, model and effort kept (A3)
    let full = Ok(Policy {
        revision: 2,
        grants: vec![g("codex", "full", None)],
    });
    let own: Vec<String> = [
        "-s",
        "workspace-write",
        "-m",
        "gpt-5",
        "-c",
        "model_reasoning_effort=high",
        "-c",
        "sandbox_mode=read-only",
        "--full-auto",
    ]
    .map(String::from)
    .into();
    for os in ["macos", "windows", "linux"] {
        let e = resolve(&full, &host(os, i, r), "codex", "", &own);
        assert_eq!(
            (e.codex.unwrap(), e.profile, e.refused),
            (
                [
                    "-s",
                    "danger-full-access",
                    "-m",
                    "gpt-5",
                    "-c",
                    "model_reasoning_effort=high"
                ]
                .map(String::from)
                .to_vec(),
                None,
                None
            )
        );
    }
    let e = resolve(&p, &host("macos", i, r), "codex", "", &ws);
    assert_eq!(
        e.codex.unwrap(),
        ["-s", "danger-full-access", "--add-dir", "/p/.git"]
    );
    assert!(
        e.profile
            .unwrap()
            .contains(&format!("(subpath {})", sb(&r.join(".taskq"))))
    );
    assert!(
        resolve(&p, &host("linux", i, r), "codex", "", &ws)
            .refused
            .unwrap()
            .contains("enforcement unknown")
    );
    // R9/#2159: a CLI Workspace grant cannot authorize an uncontained visible app turn.
    for os in ["macos", "windows"] {
        let app = Ok(Policy {
            revision: 33,
            grants: vec![g("codex-app", "workspace", Some("/p"))],
        });
        assert!(
            resolve(&app, &host(os, i, r), "codex-app", "", &[])
                .refused
                .unwrap()
                .contains("enforcement unknown")
        );
        let cli = Ok(Policy {
            revision: 33,
            grants: vec![g("codex", "full", Some("/p"))],
        });
        assert!(
            resolve(&cli, &host(os, i, r), "codex-app", "", &[])
                .refused
                .is_some()
        );
    }
}

// Chain: R9 / § 8 Access, #2145 A2/A4. Risk: CLI state is blocked or writable settings bypass tool policy.
#[test]
fn antigravity_workspace_state_and_protected_settings() {
    let h = host("macos", Path::new("/i"), Path::new("/p"));
    let p = Ok(Policy {
        revision: 1,
        grants: vec![g("antigravity", "workspace", None)],
    });
    let e = resolve(&p, &h, "antigravity", "", &[]);
    assert!(e.refused.is_none());
    let profile = e.profile.unwrap();
    for path in [".gemini/antigravity", ".gemini/antigravity-cli"] {
        assert!(
            profile.contains(&format!("(subpath {})", sb(&h.home.join(path)))),
            "{path}"
        );
    }
    assert!(profile.contains(r#"(deny file-write* (regex #"/\.gemini/(config/hooks\.json|antigravity-cli/settings\.json)$"))"#));
    let other = resolve(
        &Ok(Policy {
            revision: 1,
            grants: vec![g("claude", "workspace", None)],
        }),
        &h,
        "claude",
        "",
        &[],
    )
    .profile
    .unwrap();
    assert!(!other.contains(&format!(
        "(subpath {})",
        sb(&h.home.join(".gemini/antigravity-cli"))
    )));
}

// Chain: § 8 Access store. Risk: a grant loses its revision, or a corrupt store is replaced by a grant.
#[test]
fn grants_write_with_revision_and_refuse_workers() {
    let install = std::env::temp_dir().join(format!("taskq-policy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&install);
    std::fs::create_dir_all(&install).unwrap();
    let p = grant(&install, "claude", "workspace", None, "op", 5).unwrap();
    assert_eq!((p.revision, p.grants.len()), (1, 1));
    let p = grant(&install, "claude", "none", None, "op", 6).unwrap();
    assert_eq!((p.revision, p.grants.len()), (2, 0));
    assert!(grant(&install, "claude", "admin", None, "op", 7).is_err());
    std::fs::write(install.join(FILE), "{broken").unwrap();
    assert!(grant(&install, "claude", "full", None, "op", 8).is_err());
    assert_eq!(
        std::fs::read_to_string(install.join(FILE)).unwrap(),
        "{broken"
    );
    let _ = std::fs::remove_dir_all(&install);
}

// Chain: § 8 Access N/N-1 rollback against 4.1.11 (same coreq v0.5.0 launcher). Risk: an unhealthy release rolls
// back to 4.1.11, which ignores grants and runs legacy flags, or the pin moves `previous` without a grant.
#[test]
fn grants_keep_the_launcher_above_the_policy_floor() {
    let install = std::env::temp_dir().join(format!("taskq-floor-{}", std::process::id()));
    // the launcher without the incident hook: a test writes no incident
    let t = coreq::Tool {
        install: install.clone(),
        fault: None,
        ..coreq::Tool::new(
            "taskq",
            "TaskQ",
            crate::platform::RELEASE_REPO,
            crate::platform::VERSION,
        )
    };
    let new = "4.2.0-dev.1760000000";
    for granted in [false, true] {
        let _ = std::fs::remove_dir_all(&install);
        std::fs::create_dir_all(install.join("releases").join(new)).unwrap();
        std::fs::write(install.join("current"), new).unwrap();
        std::fs::write(install.join("previous"), "4.1.11").unwrap();
        if granted {
            grant(&install, "claude", "workspace", None, "op", 1).unwrap();
        }
        assert_eq!(pin_rollback(&install, new).is_some(), granted);
        let picks: Vec<String> = (0..3)
            .map(|_| coreq::launcher::pick(&t, true).unwrap())
            .collect();
        assert_eq!(
            picks[2],
            if granted { new } else { "4.1.11" },
            "granted {granted}"
        );
        assert_eq!(
            std::fs::read_to_string(install.join("previous"))
                .unwrap()
                .trim(),
            if granted { new } else { "4.1.11" }
        );
    }
    // a store that reads unknown pins too; a newer previous stays
    std::fs::write(install.join(FILE), "{broken").unwrap();
    std::fs::write(install.join("previous"), "4.1.11").unwrap();
    assert!(pin_rollback(&install, new).is_some());
    std::fs::write(install.join("previous"), "4.2.0-dev.1").unwrap();
    assert!(pin_rollback(&install, new).is_none());
    let _ = std::fs::remove_dir_all(&install);
}

// Chain: § 8 Access OS boundary on macOS (A2). Risk: a worker under any non-Full mode writes, replaces or
// links over worker-policy.json; Workspace cannot write outside its set; Restricted cannot write the root.
#[cfg(target_os = "macos")]
#[test]
fn seatbelt_keeps_workers_out_of_the_policy_store() {
    use std::process::Command;
    // Probe before negative assertions: a nested sandbox denial must not look like policy enforcement.
    let probe = Command::new("sandbox-exec")
        .args(["-p", "(version 1)(allow default)", "/usr/bin/true"])
        .output()
        .unwrap();
    if !probe.status.success()
        && String::from_utf8_lossy(&probe.stderr).contains("Operation not permitted")
    {
        use std::io::Write;
        writeln!(std::io::stderr(), "sandbox-blocked: seatbelt_keeps_workers_out_of_the_policy_store: Operation not permitted").unwrap();
        return;
    }
    assert!(
        probe.status.success(),
        "sandbox-exec probe: {}",
        String::from_utf8_lossy(&probe.stderr)
    );
    // outside the OS temp dir, which every preset may write
    let t = canon(Path::new(env!("CARGO_MANIFEST_DIR")))
        .join("target")
        .join(format!("seatbelt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&t);
    let (install, root, home, other) = (
        t.join("install"),
        t.join("root"),
        t.join("home"),
        t.join("other"),
    );
    for d in [&install, &root, &home, &other] {
        std::fs::create_dir_all(d).unwrap();
    }
    let path = install.join(FILE);
    std::fs::write(&path, "{\"revision\": 1}\n").unwrap();
    let run = |profile: &str, script: &str| {
        Command::new("sandbox-exec")
            .args(["-p", profile, "/bin/sh", "-c", script])
            .env("P", &path)
            .env("I", &install)
            .env("R", &root)
            .env("O", &other)
            .env("T", &t)
            .env("H", &home)
            .status()
            .unwrap()
            .success()
    };
    let h = |os| Host {
        os,
        install: &install,
        root: &root,
        home: &home,
    };
    let none = Ok(Policy::default());
    let legacy = resolve(&none, &h("macos"), "claude", "dontAsk", &[])
        .profile
        .unwrap();
    let ws = resolve(
        &Ok(Policy {
            revision: 1,
            grants: vec![g("claude", "workspace", None)],
        }),
        &h("macos"),
        "claude",
        "dontAsk",
        &[],
    )
    .profile
    .unwrap();
    let ro = resolve(
        &Ok(Policy {
            revision: 1,
            grants: vec![g("claude", "restricted", None)],
        }),
        &h("macos"),
        "claude",
        "dontAsk",
        &[],
    )
    .profile
    .unwrap();
    for p in [&legacy, &ws, &ro] {
        assert!(
            !run(p, "echo '{\"revision\":9,\"grants\":[]}' > \"$P\""),
            "write"
        );
        assert!(
            !run(p, "x=$(mktemp) && echo x > \"$x\" && mv \"$x\" \"$P\""),
            "rename over"
        );
        assert!(!run(p, "rm \"$P\""), "delete");
        assert!(!run(p, "echo x > \"$I/new\""), "create in install");
        assert!(
            !run(
                p,
                "ln -s \"$I\" \"$R/l\" 2>/dev/null; echo x > \"$R/l/new\""
            ),
            "symlink"
        );
        assert!(!run(p, "ln \"$P\" \"$T/h\""), "hard link");
        assert!(
            run(p, "cat \"$P\" >/dev/null && mktemp >/dev/null"),
            "read and temp"
        );
    }
    assert!(
        run(&legacy, "echo x > \"$O/a\"")
            && !run(
                &legacy,
                "mkdir -p \"$O/.codex\" && echo x > \"$O/.codex/config.toml\""
            )
    );
    assert!(
        !run(
            &ws,
            "mkdir -p \"$R/.codex\"; echo x > \"$R/.codex/config.toml\""
        ) && run(&ws, "echo x > \"$R/.codex/state\"")
    );
    let first = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .find(|d| d.is_absolute() && d.is_dir())
        .unwrap();
    assert!(legacy.contains(&sb(&canon(&first))), "PATH dirs closed");
    assert!(run(&ws, "echo x > \"$R/a\"") && !run(&ws, "echo x > \"$O/a\""));
    assert!(!run(&ro, "echo x > \"$R/b\"") && !run(&ro, "echo x > \"$O/b\""));
    // #2145 A2/A4: CLI state remains writable; protected configuration cannot be replaced or deleted.
    for d in [
        ".gemini/antigravity",
        ".gemini/antigravity-cli",
        ".gemini/config",
    ] {
        std::fs::create_dir_all(home.join(d)).unwrap();
    }
    let agy = resolve(
        &Ok(Policy {
            revision: 1,
            grants: vec![g("antigravity", "workspace", None)],
        }),
        &h("macos"),
        "antigravity",
        "",
        &[],
    )
    .profile
    .unwrap();
    for d in ["antigravity", "antigravity-cli"] {
        assert!(run(&agy, &format!("echo state > \"$H/.gemini/{d}/state\"")));
    }
    assert!(!run(&agy, "echo x > \"$H/.gemini/ungranted\""));
    for name in ["config/hooks.json", "antigravity-cli/settings.json"] {
        let protected = home.join(".gemini").join(name);
        std::fs::write(&protected, "invented original").unwrap();
        for profile in [&legacy, &agy] {
            assert!(!run(
                profile,
                &format!("echo changed > \"$H/.gemini/{name}\"")
            ));
            assert!(!run(
                profile,
                &format!(
                    "echo changed > \"$R/replacement\" && mv \"$R/replacement\" \"$H/.gemini/{name}\""
                )
            ));
            assert!(!run(profile, &format!("rm \"$H/.gemini/{name}\"")));
            assert_eq!(
                std::fs::read_to_string(&protected).unwrap(),
                "invented original"
            );
        }
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "{\"revision\": 1}\n"
    );
    let _ = std::fs::remove_dir_all(&t);
}

#[cfg(unix)]
fn git_project(name: &str) -> PathBuf {
    let t = canon(Path::new(env!("CARGO_MANIFEST_DIR")))
        .join("target")
        .join(format!("codex-git-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&t);
    std::fs::create_dir_all(t.join("root/.git")).unwrap();
    std::fs::create_dir_all(t.join("install")).unwrap();
    t
}

// Chain: #2449 A2 and A3, § 8 Access -> Mac legacy Codex arguments -> `taskq doctor` codex-git. Risk: Codex keeps
// `<cwd>/.git` read-only, so Git fails in linked worktrees, and a wider root or a planted link would open config,
// hooks, another project or `<install>`. Protection: only named real Git paths are added; a launch without them is named.
#[cfg(unix)]
#[test]
fn mac_legacy_codex_adds_only_the_own_git_dir_and_doctor_names_a_launch_without_it() {
    let t = git_project("args");
    let (root, install) = (t.join("root"), t.join("install"));
    let h = host("macos", &install, &root);
    let ws = ["-s".to_string(), "workspace-write".into()];
    let git = root.join(".git");
    let e = resolve(&Ok(Policy::default()), &h, "codex", "", &ws);
    let added = format!(
        "sandbox_workspace_write.writable_roots={}",
        json!(GIT_WRITES.map(|n| git.join(n).to_string_lossy().into_owned()))
    );
    assert_eq!(
        e.codex.as_deref().unwrap(),
        ["-s", "workspace-write", "-c", added.as_str()],
        "exactly the Git paths"
    );
    assert!(
        GIT_WRITES
            .iter()
            .all(|n| !["config", "hooks", "info"].contains(n)),
        "the service runs these unsandboxed"
    );
    assert_eq!(e.grant, "none (taskq.json is a request)", "still no grant");
    assert_eq!(git_metadata_gap(&e, &h), None);

    let refused = resolve(
        &Ok(Policy::default()),
        &h,
        "codex",
        "",
        &[
            "-s".into(),
            "workspace-write".into(),
            "-c".into(),
            "sandbox_workspace_write.writable_roots=[\"/\"]".into(),
        ],
    );
    assert!(
        refused.refused.is_some() && refused.codex.is_none(),
        "a requested root stays refused"
    );
    assert!(
        resolve(
            &Ok(Policy::default()),
            &host("linux", &install, &root),
            "codex",
            "",
            &ws
        )
        .codex
        .is_none(),
        "macOS only"
    );

    let ro = resolve(
        &Ok(Policy::default()),
        &h,
        "codex",
        "",
        &["-s".into(), "read-only".into()],
    );
    assert!(ro.codex.is_none(), "read-only stays read-only");
    assert!(
        git_metadata_gap(&ro, &h)
            .unwrap()
            .contains("Mac codex legacy launch cannot write")
    );
    let grant = |mode| {
        resolve(
            &Ok(Policy {
                revision: 1,
                grants: vec![g("codex", mode, None)],
            }),
            &h,
            "codex",
            "",
            &ws,
        )
    };
    assert_eq!(git_metadata_gap(&grant("workspace"), &h), None);
    assert_eq!(git_metadata_gap(&grant("full"), &h), None);
    assert!(git_metadata_gap(&grant("restricted"), &h).is_some());
    assert_eq!(
        git_metadata_gap(
            &resolve(&Ok(Policy::default()), &h, "claude", "dontAsk", &[]),
            &h
        ),
        None,
        "Codex only"
    );

    // a pointer or a planted link never becomes a root, and the launch is named
    std::fs::create_dir_all(git.join("logs")).unwrap();
    std::fs::remove_dir(git.join("logs")).unwrap();
    std::os::unix::fs::symlink(&install, git.join("refs")).unwrap();
    let planted = resolve(&Ok(Policy::default()), &h, "codex", "", &ws);
    assert!(planted.codex.is_none() && git_metadata_gap(&planted, &h).is_some());
    std::fs::remove_file(git.join("refs")).unwrap();
    std::fs::remove_dir(&git).unwrap();
    std::os::unix::fs::symlink(&install, &git).unwrap();
    let linked = resolve(&Ok(Policy::default()), &h, "codex", "", &ws);
    assert!(linked.codex.is_none() && git_metadata_gap(&linked, &h).is_some());
    assert!(git_metadata_gap(&grant("workspace"), &h).is_some());
    std::fs::remove_file(&git).unwrap();
    std::fs::write(&git, "gitdir: elsewhere\n").unwrap();
    let pointer = resolve(&Ok(Policy::default()), &h, "codex", "", &ws);
    assert!(pointer.codex.is_none() && git_metadata_gap(&pointer, &h).is_some());
    std::fs::remove_file(&git).unwrap();
    assert_eq!(
        git_metadata_gap(&resolve(&Ok(Policy::default()), &h, "codex", "", &ws), &h),
        None,
        "no Git here, nothing to write"
    );
    let _ = std::fs::remove_dir_all(&t);
}

// Chain: #2449 A1 and A2, real Git under both Mac sandboxes. Risk: index.lock and FETCH_HEAD fail in a linked
// worktree, or the fix also opens another project's `.git` or `<install>`. Protection: fetch, commit, rebase and push
// pass under legacy Codex arguments and the Workspace profile; both other paths stay refused.
#[cfg(target_os = "macos")]
#[test]
fn mac_codex_runs_git_in_a_linked_worktree_and_stays_out_of_other_git_dirs() {
    use std::process::Command;
    let probe = Command::new("sandbox-exec")
        .args(["-p", "(version 1)(allow default)", "/usr/bin/true"])
        .output()
        .unwrap();
    if !probe.status.success() {
        use std::io::Write;
        writeln!(
            std::io::stderr(),
            "sandbox-blocked: mac_codex_runs_git_in_a_linked_worktree: {}",
            String::from_utf8_lossy(&probe.stderr)
        )
        .unwrap();
        return;
    }
    let t = git_project("real");
    let (install, other) = (t.join("install"), t.join("other"));
    std::fs::create_dir_all(other.join(".git")).unwrap();
    let home = t.join("home");
    std::fs::create_dir_all(&home).unwrap();
    // a push target the sandboxes may write: the OS temp dir, like the network for a real remote
    let temp = PathBuf::from("/private/tmp").join(format!("codex-git-{}", std::process::id()));
    std::fs::create_dir_all(&temp).unwrap();
    let git = |dir: &Path, args: &[&str]| {
        let o = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    };
    // the service launches Codex in the root; Codex masks the gitdir of a pointer `.git` in its cwd
    let script = "cd .worktrees/w && git fetch origin && echo w >> f && git add f && git commit -qm w && git rebase -q origin/main && git push -q origin HEAD:refs/heads/$B && git pack-refs --all && git worktree add -q -b x$B ../x \
            && ! touch \"$O/.git/x\" 2>/dev/null && ! touch \"$I/x\" 2>/dev/null";
    let prepare = |branch: &str| -> PathBuf {
        let root = t.join(format!("root-{branch}"));
        std::fs::create_dir_all(&root).unwrap();
        let origin = temp.join(format!("{branch}.git"));
        git(&temp, &["init", "-q", "--bare", origin.to_str().unwrap()]);
        git(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("f"), "1\n").unwrap();
        git(&root, &["add", "f"]);
        git(&root, &["commit", "-qm", "one"]);
        git(
            &root,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git(&root, &["push", "-q", "origin", "main"]);
        git(&root, &["config", "user.name", "t"]);
        git(&root, &["config", "user.email", "t@t"]);
        git(
            &root,
            &["worktree", "add", "-q", "-b", branch, ".worktrees/w"],
        );
        std::fs::write(root.join("g"), "2\n").unwrap();
        git(&root, &["add", "g"]);
        git(&root, &["commit", "-qm", "two"]);
        git(&root, &["push", "-q", "origin", "main"]);
        root
    };
    let run = |c: &mut Command, root: &Path, branch: &str| {
        let o = c
            .arg("/bin/sh")
            .arg("-c")
            .arg(script)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("B", branch)
            .env("R", root)
            .env("O", &other)
            .env("I", &install)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "{branch}: {}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(!other.join(".git/x").exists() && !install.join("x").exists());
        git(root, &["fetch", "-q", "origin"]);
        git(
            root,
            &[
                "merge-base",
                "--is-ancestor",
                "origin/main",
                &format!("origin/{branch}"),
            ],
        );
    };

    let root = prepare("workspace");
    let h = Host {
        os: "macos",
        install: &install,
        root: &root,
        home: &home,
    };
    let profile = resolve(
        &Ok(Policy {
            revision: 1,
            grants: vec![g("codex", "workspace", None)],
        }),
        &h,
        "codex",
        "",
        &["-s".into(), "workspace-write".into()],
    )
    .profile
    .unwrap();
    run(
        Command::new("sandbox-exec").args(["-p", &profile]),
        &root,
        "workspace",
    );

    let root = prepare("legacy");
    let h = Host {
        os: "macos",
        install: &install,
        root: &root,
        home: &home,
    };
    let args = resolve(
        &Ok(Policy::default()),
        &h,
        "codex",
        "",
        &["-s".into(), "workspace-write".into()],
    )
    .codex
    .unwrap();
    let codex = Command::new("codex").arg("--version").output();
    if codex.is_ok_and(|o| o.status.success()) {
        let mut c = Command::new("codex");
        c.args([
            "sandbox",
            "-c",
            "sandbox_mode=workspace-write",
            "-c",
            &args[3],
            "--",
        ]);
        run(&mut c, &root, "legacy");
        let closed = "! echo x >> .git/config && ! touch .git/hooks/post-commit && ! touch .git/info/attributes";
        let o = Command::new("codex")
            .args([
                "sandbox",
                "-c",
                "sandbox_mode=workspace-write",
                "-c",
                &args[3],
                "--",
                "/bin/sh",
                "-c",
                closed,
            ])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "config, hooks and info stay closed: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    } else {
        use std::io::Write;
        writeln!(
            std::io::stderr(),
            "no codex CLI: legacy half of mac_codex_runs_git_in_a_linked_worktree not run"
        )
        .unwrap();
    }
    let _ = std::fs::remove_dir_all(&t);
    let _ = std::fs::remove_dir_all(&temp);
}
