//! § 6 setup check (#1938, [detail](../docs/design/setup-check.md#contract-detail)): read-only items from facts the
//! caller gathered; each failed item names one plain-language gap and its exact fix.

use crate::runtime;
use serde_json::{Value, json};
use std::path::PathBuf;

/// A board probe: verified, a gap with its reason, or unknown with why (R12).
#[derive(Clone, Debug, PartialEq)]
pub enum Probe { Ok, Gap(String), Unknown(String) }

/// One runtime in use here: its `host.json` limit is above 0, or it is the default runtime.
#[derive(Clone, Debug, Default)]
pub struct Runtime {
    pub name: String,
    /// Why the CLI does not resolve on the service PATH.
    pub missing: Option<String>,
    /// § 5 `agents` row `login`; `None`: no probe yet.
    pub login: Option<String>,
    /// § 8 Access refusal.
    pub refused: Option<String>,
    /// An operator grant exists for it here.
    pub granted: bool,
}

#[derive(Clone, Debug)]
pub struct Facts {
    pub os: String,
    pub root: PathBuf,
    pub host: String,
    pub git: bool,
    /// `workspace: external`: whether the project has its `AGENTS.md`; `None`: taskq worktrees.
    pub external: Option<bool>,
    pub runtimes: Vec<Runtime>,
    pub codex_dir: bool,
    pub labelled: bool,
    /// Open tasks without a `host-*` label; `None`: not read.
    pub unlabelled: Option<Vec<u64>>,
    /// GitHub `<install>/token`; `None`: another board.
    pub token: Option<Result<(), String>>,
    /// The preset, or why `publish` and `delivery` disagree.
    pub delivery: Result<crate::config::Delivery, String>,
    /// `None`: the preset does not publish by PR.
    pub pr_route: Option<Probe>,
    /// `host_setup` and whether its last `service install` run succeeded; `None`: not declared.
    pub host_setup: Option<(String, bool)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    /// `ok`, `gap` or `unknown`.
    pub status: &'static str,
    pub text: String,
    pub fix: Option<String>,
}

impl Item {
    pub(crate) fn ok(id: impl Into<String>, text: impl Into<String>) -> Self { Item { id: id.into(), status: "ok", text: text.into(), fix: None } }
    pub(crate) fn gap(id: impl Into<String>, text: impl Into<String>, fix: impl Into<String>) -> Self { Item { id: id.into(), status: "gap", text: text.into(), fix: Some(fix.into()) } }
    pub(crate) fn unknown(id: impl Into<String>, text: impl Into<String>, fix: impl Into<String>) -> Self { Item { id: id.into(), status: "unknown", text: text.into(), fix: Some(fix.into()) } }
    pub fn json(&self) -> Value { json!({"id": self.id, "status": self.status, "text": self.text, "fix": self.fix}) }
}

pub fn items(f: &Facts) -> Vec<Item> {
    let root = f.root.display();
    let mut out = vec![if f.git { Item::ok("checkout", "project checkout exists") }
        else { Item::gap("checkout", format!("{root} is not a Git checkout, so no worker can start"), "clone the repository into this folder or run `git init` there") }];
    out.push(match f.external {
        None => Item::ok("workspace", "workers use taskq worktrees under .worktrees"),
        Some(true) => Item::ok("workspace", "workspace external, path from AGENTS.md"),
        Some(false) => Item::gap("workspace", "workspace is external but the project has no AGENTS.md naming the worker path", "add AGENTS.md with the workspace path workers use"),
    });
    let codex_dir = f.os == "linux" && f.runtimes.iter().any(|r| runtime::needs_codex_dir(&f.os, &r.name)) && !f.codex_dir;
    for r in &f.runtimes {
        let rt = &r.name;
        out.push(match &r.missing {
            Some(why) => Item::gap(format!("cli {rt}"), format!("{why}; {rt} tasks never start"), format!("install the {rt} CLI so the service finds it")),
            None => Item::ok(format!("cli {rt}"), format!("{rt} CLI found")),
        });
        if r.missing.is_none() {
            out.push(match r.login.as_deref() {
                Some("logged in") => Item::ok(format!("login {rt}"), format!("{rt} logged in")),
                Some("not logged in") => Item::gap(format!("login {rt}"), format!("{rt} is not logged in; its tasks never start"), runtime::relogin(rt)),
                _ => Item::unknown(format!("login {rt}"), format!("{rt} login not probed yet"), "run `taskq agents`"),
            });
        }
        // the missing .codex directory is the root cause: name it once, below
        let from_codex_dir = codex_dir && runtime::needs_codex_dir(&f.os, rt) && r.refused.as_deref().is_some_and(|x| x.contains(".codex"));
        out.push(match &r.refused {
            Some(why) if !from_codex_dir => Item::gap(format!("access {rt}"), format!("access refused for {rt} ({why}); its launches are held"),
                if r.granted { "resolve the refusal, then read `taskq service permissions`".to_string() }
                else { format!("`taskq service permissions --runtime {rt} --mode {} --scope project`", runtime::grant_mode(&f.os, rt)) }),
            _ => Item::ok(format!("access {rt}"), format!("{rt} access granted")),
        });
    }
    if codex_dir {
        out.push(Item::gap("codex-dir", format!("{root}/.codex is missing; the Linux Codex sandbox refuses to start"), format!("`mkdir {root}/.codex`")));
    }
    if let Some((cmd, done)) = &f.host_setup { out.push(host_setup_item(&f.root, cmd, *done)); }
    if f.labelled {
        out.push(match &f.unlabelled {
            None => Item::unknown("host-label", "claim labelled: open tasks not read", "run `taskq service check` again when the board answers"),
            Some(v) if v.is_empty() => Item::ok("host-label", "claim labelled: every open task names a host"),
            Some(v) => Item::gap("host-label", format!("this host claims only host-{} tasks and {} open task(s) have no host-* label ({}); they never start", f.host, v.len(),
                v.iter().take(5).map(|n| format!("#{n}")).collect::<Vec<_>>().join(", ")),
                format!("`taskq service capacity --host {} --claim any`, or label the tasks host-{}", f.host, f.host)),
        });
    }
    match &f.token {
        None => {}
        Some(Ok(())) => out.push(Item::ok("token", "board token installed")),
        Some(Err(why)) => out.push(Item::gap("token", format!("board token {why}; the service cannot read the board"), "the owner installs a GitHub token in the taskq install folder (never `gh auth login`)")),
    }
    out.push(match &f.delivery {
        Ok(p) => Item::ok("delivery", format!("delivery preset {p}")),
        Err(why) => Item::gap("delivery", why.trim_start_matches("taskq.json: ").to_string(), "remove `publish` from taskq.json, or set it to the route of `delivery.preset`"),
    });
    match &f.pr_route {
        None => {}
        Some(Probe::Ok) => out.push(Item::ok("pr-route", "main accepts PR publication")),
        Some(Probe::Gap(why)) => out.push(Item::gap("pr-route", format!("{why}, so PR results never merge"), "set `delivery.preset` to `direct` in taskq.json, or add a merge queue or strict main protection")),
        Some(Probe::Unknown(why)) => out.push(Item::unknown("pr-route", format!("PR route unknown: {why}"), "run `taskq service check` again when the board answers")),
    }
    out
}

/// `<root>/.taskq/host-setup`: the `host_setup` command of the last successful `service install` run.
/// `ponytail:` a stamp, not a probe of the setup's effect; a project-defined probe if setups get undone by hand.
pub fn host_setup_stamp(root: &std::path::Path) -> PathBuf { root.join(".taskq").join("host-setup") }

/// The `host-setup` item; `taskq doctor` prints it too.
pub fn host_setup_item(root: &std::path::Path, cmd: &str, done: bool) -> Item {
    if done { return Item::ok("host-setup", format!("host setup `{cmd}` done")); }
    Item::gap("host-setup", format!("host setup `{cmd}` has not run on this host; workers cannot do it from their sandbox"),
        format!("run `taskq service install` in {}", root.display()))
}

/// Whether `cmd` ran successfully here: the stamp names exactly it, so a changed command runs again.
pub fn host_setup_done(root: &std::path::Path, cmd: &str) -> bool {
    std::fs::read_to_string(host_setup_stamp(root)).is_ok_and(|s| s.trim() == cmd)
}

/// Text output: `Gap:`/`Unknown:` plus `Next:` per open item, or one `Ready:` line.
pub fn lines(items: &[Item]) -> Vec<String> {
    let open: Vec<String> = items.iter().filter(|i| i.status != "ok").flat_map(|i| [
        format!("{}: {}", if i.status == "gap" { "Gap" } else { "Unknown" }, i.text),
        format!("Next: {}", i.fix.as_deref().unwrap_or_default()),
    ]).collect();
    if open.is_empty() { vec!["Ready: setup check passed".into()] } else { open }
}

pub fn json(project: &str, items: &[Item]) -> Value {
    json!({"project": project, "items": items.iter().map(Item::json).collect::<Vec<_>>()})
}

/// `service status` and R6: gap lines of a `status.json` `setup` value; unknowns stay for `service check`.
pub fn status_lines(setup: &Value) -> Vec<String> {
    setup["items"].as_array().into_iter().flatten().filter(|i| i["status"] == "gap").flat_map(|i| [
        format!("Setup gap: {}", i["text"].as_str().unwrap_or("?")),
        format!("Next: {}", i["fix"].as_str().unwrap_or("run `taskq service check`")),
    ]).collect()
}

#[cfg(test)]
#[path = "check_tests.rs"]
mod tests;
