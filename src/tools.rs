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

/// The command line of `cmd` for a diagnostic: program file name, then its arguments.
fn shown(cmd: &Command) -> String {
    let program = Path::new(cmd.get_program()).file_name().unwrap_or(cmd.get_program()).to_string_lossy().into_owned();
    std::iter::once(program).chain(cmd.get_args().map(|a| a.to_string_lossy().into_owned())).collect::<Vec<_>>().join(" ")
}

/// Runs an external tool call: its stdout, else the command and its error, with the exit status and stderr of a
/// nonzero exit. A failed call never reads as an empty result ([host tools](../docs/design/host-service.md#host-tools)).
pub fn run(name: &'static str, cmd: &mut Command, budget: Duration) -> Result<Vec<u8>, String> {
    let line = shown(cmd);
    match coreq::process::output(name, cmd, budget) {
        Ok(o) => Ok(o.stdout),
        Err(coreq::process::Failure::Exit(o)) => {
            let err = String::from_utf8_lossy(&o.stderr).lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("; ");
            Err(format!("`{line}` failed: {}: {}", o.status, if err.is_empty() { "no error output" } else { &err }))
        }
        Err(e) => Err(format!("`{line}` failed: {e}")),
    }
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

/// `<bin> --version` from the tool's own directory. #2329: a timeout ends only the direct child, so a descendant of a
/// command-script tool outlives it; on Windows that descendant's working directory stays in use and undeletable, so
/// the probe never inherits the caller's project or install directory.
fn probe(bin: &Path) -> Command {
    let mut probe = crate::platform::command(bin);
    probe.arg("--version");
    if let Some(dir) = bin.parent().filter(|dir| dir.is_absolute()) { probe.current_dir(dir); }
    probe
}

/// `<tool> --version` through `bin`: the found version, or the failed command with its error (never empty).
fn version(tool: &str, bin: &Path) -> Result<String, String> {
    let cmd = format!("`{tool} --version`");
    let out = crate::platform::output("tool.version", &mut probe(bin), Duration::from_secs(10)).map_err(|e| format!("{cmd} failed: {e}"))?;
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
        Ok(v) if min.is_empty() => Ok(format!("{tool}: {v}; minimum version unqualified")),
        Ok(v) => Ok(format!("{tool}: {v} (minimum {min}) PASS")),
    }
}

/// Whether any project registered on this host, or `root`, uses a GitHub board; unknown without either.
pub fn github(install: &Path, root: Option<&Path>) -> Option<bool> {
    let roots: Vec<_> = root.map(Path::to_path_buf).into_iter().chain(crate::host::projects(install).unwrap_or_default()).collect();
    let boards: Vec<String> = roots.iter().filter_map(|r| crate::config::ProjectFile::of(r).read().ok())
        .map(|v| v["board"].as_str().unwrap_or("github").to_string()).collect();
    (!boards.is_empty()).then(|| boards.iter().any(|b| b == "github"))
}

/// Every tool row of this host: (line, failed). Not needed tools (gh on a GitLab-only host) are left out.
pub fn rows(install: &Path, root: Option<&Path>) -> Vec<(String, bool)> {
    let default_runtime = crate::host::Policy::read(install).ok().map(|p| p.default_runtime.name());
    let selected = default_runtime.and_then(|rt| crate::labels::select(&[], false, rt, |candidate| {
        crate::runtime::missing_cli(candidate).map_or_else(|| Ok(candidate.to_string()), |why| Err(("capability", why)))
    }).ok().map(|(rt, _)| rt));
    let github = github(install, root);
    needs(github, selected.as_deref().or(default_runtime)).into_iter().map(|(n, req)| {
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
#[path = "tools_tests.rs"]
mod tests;
