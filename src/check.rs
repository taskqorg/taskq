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
    pub delivery: Result<String, String>,
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
    fn ok(id: impl Into<String>, text: impl Into<String>) -> Self { Item { id: id.into(), status: "ok", text: text.into(), fix: None } }
    fn gap(id: impl Into<String>, text: impl Into<String>, fix: impl Into<String>) -> Self { Item { id: id.into(), status: "gap", text: text.into(), fix: Some(fix.into()) } }
    fn unknown(id: impl Into<String>, text: impl Into<String>, fix: impl Into<String>) -> Self { Item { id: id.into(), status: "unknown", text: text.into(), fix: Some(fix.into()) } }
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
mod tests {
    use super::*;

    fn facts(os: &str) -> Facts {
        Facts {
            os: os.into(), root: PathBuf::from("/p"), host: "h".into(), git: true, external: None,
            runtimes: vec![Runtime { name: "claude".into(), login: Some("logged in".into()), granted: true, ..Default::default() }],
            codex_dir: true, labelled: false, unlabelled: None, token: Some(Ok(())), delivery: Ok("auto-merge".into()), pr_route: Some(Probe::Ok),
            host_setup: None,
        }
    }

    fn gaps(f: &Facts) -> Vec<(String, String)> {
        items(f).into_iter().filter(|i| i.status == "gap").map(|i| (i.id, i.fix.unwrap())).collect()
    }

    // Chain: § 6 setup check (#1938 A2/A4). Risk: a ready project reports a gap, so owners learn to ignore it.
    #[test]
    fn a_ready_project_prints_one_ready_line() {
        let f = facts("macos");
        assert!(items(&f).iter().all(|i| i.status == "ok"), "{:?}", items(&f));
        assert_eq!(lines(&items(&f)), ["Ready: setup check passed"]);
        assert_eq!(json("p", &items(&f))["items"].as_array().unwrap().len(), items(&f).len(), "--json keeps ok items too");
    }

    // Defect (2026-10-30, A6): codex limit 4 without a grant held every Codex launch and the reviewer never spawned,
    // silently. Protection: the refused runtime is a gap whose fix is the exact grant command.
    #[test]
    fn codex_in_use_without_a_grant_names_the_grant_command() {
        let mut f = facts("windows");
        f.runtimes.push(Runtime { name: "codex".into(), login: Some("logged in".into()),
            refused: Some("no operator grant and no policy store protection for codex on windows; grant a preset".into()), ..Default::default() });
        assert_eq!(gaps(&f), [("access codex".to_string(), "`taskq service permissions --runtime codex --mode workspace --scope project`".to_string())]);
        let l = lines(&items(&f));
        assert!(l[0].starts_with("Gap: access refused for codex") && l[0].ends_with("its launches are held"), "{l:?}");
        assert!(l[1].starts_with("Next: `taskq service permissions"), "{l:?}");
        f.runtimes[0].refused = Some("full enforcement unknown".into());
        f.runtimes[0].granted = false;
        assert!(gaps(&f).iter().any(|(id, fix)| id == "access claude" && fix.contains("--mode full")), "Claude off macOS has only Full");
    }

    // Defect (2026-10-30, A6): `publish: pr` beside `delivery.preset: direct` and a private repository whose rules read
    // answers 403 left the project unable to publish. Protection: both are gaps with their owner fix.
    #[test]
    fn delivery_disagreement_and_a_refused_rules_read_are_gaps() {
        let mut f = facts("macos");
        f.delivery = Err("taskq.json: publish pr disagrees with delivery.preset direct".into());
        f.pr_route = Some(Probe::Gap("the board refuses to read main branch rules (HTTP 403)".into()));
        let g = gaps(&f);
        assert_eq!(g.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["delivery", "pr-route"]);
        assert!(g[0].1.contains("remove `publish`") && g[1].1.contains("`direct`"), "{g:?}");
        assert_eq!(items(&f).iter().find(|i| i.id == "delivery").unwrap().text, "publish pr disagrees with delivery.preset direct");
        f.pr_route = Some(Probe::Unknown("HTTP 503".into()));
        assert_eq!(items(&f).iter().find(|i| i.id == "pr-route").unwrap().status, "unknown", "R12: unverified stays unknown");
    }

    // Defect (2026-10-30, A6): on Linux a missing `<root>/.codex` refused Codex with a sandbox prerequisite error.
    // Protection: one codex-dir gap with the mkdir fix, not a misleading grant step.
    #[test]
    fn missing_codex_dir_on_linux_is_one_gap_with_mkdir() {
        let mut f = facts("linux");
        f.codex_dir = false;
        f.runtimes.push(Runtime { name: "codex".into(), login: Some("logged in".into()), granted: true,
            refused: Some("linux sandbox prerequisite /p/.codex: No such file or directory".into()), ..Default::default() });
        assert_eq!(gaps(&f), [("codex-dir".to_string(), "`mkdir /p/.codex`".to_string())]);
        f.os = "macos".into();
        f.runtimes[1].refused = None;
        assert!(gaps(&f).is_empty(), "only Linux needs the project .codex");
    }

    // Defect cs#548 (#2078 A3): the shared pre-commit hook was never installed on pds18, a worker sandbox cannot write
    // the shared .git, and attempts burned silently. Protection: a declared host setup that has not run is a gap whose
    // fix runs it outside the sandbox; a changed command needs a new run.
    #[test]
    fn host_setup_not_run_is_a_gap_until_install_stamps_it() {
        let root = std::env::temp_dir().join(format!("taskq-host-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".taskq")).unwrap();
        let cmd = "scripts/install-git-hooks.sh";
        let mut f = facts("linux");
        f.host_setup = Some((cmd.into(), host_setup_done(&root, cmd)));
        assert_eq!(gaps(&f), [("host-setup".to_string(), "run `taskq service install` in /p".to_string())]);
        assert!(status_lines(&json("p", &items(&f)))[0].starts_with("Setup gap: host setup `scripts/install-git-hooks.sh` has not run"));
        std::fs::write(host_setup_stamp(&root), format!("{cmd}\n")).unwrap();
        f.host_setup = Some((cmd.into(), host_setup_done(&root, cmd)));
        assert!(gaps(&f).is_empty(), "{:?}", gaps(&f));
        assert!(!host_setup_done(&root, "scripts/other.sh"), "a changed command runs again");
        f.host_setup = None;
        assert!(items(&f).iter().all(|i| i.id != "host-setup"), "undeclared: no item");
        let _ = std::fs::remove_dir_all(&root);
    }

    // Chain: § 6 setup check host items (#1938 A2). Risk: a missing CLI, login, token, checkout or host label stalls silently.
    #[test]
    fn host_items_name_each_gap() {
        let mut f = facts("macos");
        f.git = false;
        f.external = Some(false);
        f.runtimes[0].login = Some("not logged in".into());
        f.runtimes.push(Runtime { name: "codex".into(), missing: Some("codex CLI not found on the service PATH".into()), ..Default::default() });
        f.labelled = true;
        f.unlabelled = Some(vec![4, 9]);
        f.token = Some(Err("file missing".into()));
        let ids: Vec<String> = gaps(&f).into_iter().map(|g| g.0).collect();
        assert_eq!(ids, ["checkout", "workspace", "login claude", "cli codex", "host-label", "token"]);
        assert!(items(&f).iter().all(|i| i.id != "login codex"), "a missing CLI has no login to probe");
        let label = items(&f).into_iter().find(|i| i.id == "host-label").unwrap();
        assert!(label.text.contains("(#4, #9)") && label.fix.unwrap().contains("--host h --claim any"));
    }

    // Chain: § 6 status lines (#1938 A5). Risk: status hides a gap, or shows unknowns as gaps.
    #[test]
    fn status_lines_show_only_gaps() {
        let mut f = facts("macos");
        f.runtimes[0].refused = Some("policy unreadable (x)".into());
        f.pr_route = Some(Probe::Unknown("HTTP 503".into()));
        let l = status_lines(&json("p", &items(&f)));
        assert_eq!(l.len(), 2, "{l:?}");
        assert!(l[0].starts_with("Setup gap: access refused for claude") && l[1].starts_with("Next: "), "{l:?}");
        assert!(status_lines(&Value::Null).is_empty());
    }
}
