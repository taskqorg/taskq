//! Host tool requirements ([host-service](../docs/design/host-service.md#host-tools), #1856): each external tool taskq
//! runs on a host, its minimum version and the owner's fix. `taskq selftest` fails on a missing or outdated required
//! tool; service install, onboard, update and service start report it. taskq never installs a tool itself.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// One external tool: its command, minimum version, what needs it and the owner's fix step.
#[derive(Clone, Copy)]
pub struct Need {
    pub tool: &'static str,
    pub min: &'static str,
    pub needs: &'static str,
    pub fix: &'static str,
}

/// The table of docs/design/host-service.md § Host tools; a new tool, flag or API use raises its minimum here and there.
/// Runtime CLI rows come from their adapters (`runtime::clis`).
pub const TOOLS: [Need; 3] = [
    Need { tool: "curl", min: "7.55.0", needs: "update, board API and rules download", fix: "install or update curl (https://curl.se/download.html)" },
    Need { tool: "git", min: "2.17.0", needs: "worktrees, tidy, recovery and project seed", fix: "install or update Git (https://git-scm.com/downloads)" },
    Need { tool: "gh", min: "2.5.1", needs: "GitHub workers: gh pr create, gh pr checks --watch --interval", fix: "install or update GitHub CLI (https://cli.github.com)" },
];

/// This host's tools and whether each is required: curl always (update); git with a known project (taskq.md § 8: a
/// release alone needs no git); gh with a GitHub board (none known: optional; GitLab only: left out); a runtime CLI
/// when it runs host.json `default_runtime` (no host policy, as on a release runner: optional).
fn needs(github: Option<bool>, default_runtime: Option<&str>) -> Vec<(Need, bool)> {
    let [curl, git, gh] = TOOLS;
    let mut v = vec![(curl, true), (git, github.is_some())];
    if github != Some(false) { v.push((gh, github == Some(true))); }
    let main = default_runtime.map(|rt| crate::runtime::of(rt).bin);
    v.extend(crate::runtime::clis().map(|a| (Need { tool: a.bin, min: a.min, needs: a.client, fix: a.update }, main == Some(a.bin))));
    v
}

/// Leading numeric dot parts: `2.46.0.windows.1` is [2, 46, 0].
fn parts(v: &str) -> Vec<u64> {
    v.split('.').map_while(|p| p.parse().ok()).collect()
}

fn below(found: &str, min: &str) -> bool {
    let (mut a, mut b) = (parts(found), parts(min));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a < b
}

/// `<tool> --version` through `bin`: the found version, or the failed command with its error (never empty).
fn version(tool: &str, bin: &Path) -> Result<String, String> {
    let cmd = format!("`{tool} --version`");
    let out = crate::platform::output("tool.version", Command::new(bin).arg("--version"), Duration::from_secs(10)).map_err(|e| format!("{cmd} failed: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("no output");
    if !out.status.success() {
        return Err(format!("{cmd} failed: {}: {first}", out.status));
    }
    text.split_whitespace().find(|w| parts(w).first().is_some()).map(String::from).ok_or(format!("{cmd} printed no version: {first}"))
}

/// One tool's row: Ok(`<tool>: <found> (minimum <min>) PASS` or not installed, optional), else Err naming the tool,
/// found version, minimum and fix. An installed optional tool below its minimum fails too: taskq or a worker may run it.
pub fn check(need: &Need, bin: &Path, required: bool) -> Result<String, String> {
    let (tool, min) = (need.tool, need.min);
    let fail = |found: String| format!("{tool}: {found}; minimum {min} for {}; fix: {}", need.needs, need.fix);
    if !bin.is_absolute() && !bin.is_file() {
        return if required { Err(fail("not installed".into())) } else { Ok(format!("{tool}: not installed (optional)")) };
    }
    match version(tool, bin) {
        Err(e) => Err(fail(format!("version unknown ({e})"))),
        Ok(v) if below(&v, min) => Err(fail(format!("found {v}"))),
        Ok(v) => Ok(format!("{tool}: {v} (minimum {min}) PASS")),
    }
}

/// Whether any project registered on this host, or `root`, uses a GitHub board; unknown without either.
pub fn github(install: &Path, root: Option<&Path>) -> Option<bool> {
    let roots: Vec<_> = root.map(Path::to_path_buf).into_iter().chain(crate::host::projects(install).unwrap_or_default()).collect();
    let boards: Vec<String> = roots.iter().filter_map(|r| std::fs::read_to_string(r.join("taskq.json")).ok())
        .filter_map(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .map(|v| v["board"].as_str().unwrap_or("github").to_string()).collect();
    (!boards.is_empty()).then(|| boards.iter().any(|b| b == "github"))
}

/// Every tool row of this host: (line, failed). Not needed tools (gh on a GitLab-only host) are left out.
pub fn rows(install: &Path, root: Option<&Path>) -> Vec<(String, bool)> {
    let default_runtime = crate::host::Policy::read(install).ok().map(|p| p.default_runtime);
    let github = github(install, root);
    needs(github, default_runtime.as_deref()).into_iter().map(|(n, req)| {
        match check(&n, &crate::runtime::which(n.tool), req) { Ok(l) => (l, false), Err(l) => (l, true) }
    }).collect()
}

/// The owner steps for each failing tool (install, onboard, update, service start); empty when all pass.
pub fn gaps(install: &Path, root: Option<&Path>) -> Vec<String> {
    rows(install, root).into_iter().filter(|r| r.1).flat_map(|(l, _)| {
        let (what, fix) = l.split_once("; fix: ").unwrap_or((&l, "see docs/design/host-service.md#host-tools"));
        [format!("Tool: {what}"), format!("Next: {fix}; `taskq selftest` fails until then")]
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let (p, text) = if cfg!(windows) { (dir.join(format!("{name}.cmd")), format!("@echo off\r\n{}\r\n", body.replace('\n', "\r\n"))) }
            else { (dir.join(name), format!("#!/bin/sh\n{body}\n")) };
        std::fs::write(&p, text).unwrap();
        #[cfg(unix)] std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn version_compare_reads_numeric_prefix() {
        assert!(below("2.16.9", "2.17.0") && below("2.5.0", "2.5.1") && below("7.9", "7.55.0"));
        assert!(!below("2.46.0.windows.1", "2.17.0") && !below("2.17", "2.17.0") && !below("0.162.1", "0.159.3"));
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

    #[test]
    fn requirements_follow_board_and_default_runtime() {
        let req = |tool: &str, github, rt| needs(github, rt).into_iter().find(|n| n.0.tool == tool).map(|n| n.1);
        assert_eq!((req("curl", None, None), req("git", Some(false), None), req("git", None, None)), (Some(true), Some(true), Some(false)));
        assert_eq!((req("gh", Some(true), None), req("gh", None, None), req("gh", Some(false), None)), (Some(true), Some(false), None));
        assert_eq!((req("claude", None, Some("claude")), req("codex", None, Some("claude")), req("claude", None, None)), (Some(true), Some(false), Some(false)));
        assert_eq!((req("codex", None, Some("codex")), req("claude", None, Some("codex"))), (Some(true), Some(false)));
        let rows: Vec<_> = needs(None, None).into_iter().map(|n| (n.0.tool, n.0.min)).collect();
        assert_eq!(&rows[2..], [("gh", "2.5.1"), ("claude", "2.1.296"), ("codex", "0.159.3")], "one row per CLI binary");
    }
}
