//! Command line arguments and the help text (§ 5).

pub(crate) mod connection;
pub(crate) mod handin;
pub(crate) mod host_cmd;
pub(crate) mod inbox;
pub(crate) mod service_cmd;
pub(crate) mod status_cmd;
pub(crate) mod task_cmd;
pub(crate) mod text;

use crate::Res;
use serde_json::Value;
use std::fs;
use std::collections::HashMap;

pub(crate) const USAGE: &str = "taskq agents
 taskq unpin N --host H --revision R --op-id ID [--json]
 taskq reroute --from <runtime> --to <runtime> [--json]
 taskq accounts [--json] | taskq accounts enable|disable|move <runtime> <label> [<position>] | taskq accounts add <runtime> <label> | taskq accounts fleet use|avoid|clear <runtime> <id>
 taskq labels
 taskq add <title> --goal G --acceptance A [--acceptance B..] [--scope P..] [--deps N..] [--type T] [--runtime R] [--priority 0|1|2] [--host H] [--rules IDs|all] [--gate] [--conversation ID --op-id ID]
 taskq finding <title> --board URL --source installation/rule/episode --goal G --acceptance A.. [--evidence E..] [--start] [--json]
 taskq start N [M..]
 taskq deps N --board URL --deps LIST --op-id ID --revision R [--json]
 taskq ask N --text Q --option O.. --recommend K [--link URL..]
 taskq answer N --text A | taskq answer N.K [M.K..] | taskq pick N.K --text R
 Human replies: answer, steer, requeue accept --text-stdin or --text-file PATH instead of --text; attach with --file PATH (repeat) or --files-from PATH (JSON array)
 JSON files: status, answer, steer, requeue, later, close, start, failed accept --json --json-file PATH
 taskq result N --sha SHA --checks C.. --text T [--option O.. --recommend K --link URL..]
 taskq requeue N [--text T] | taskq later N [--text T] [--blocker #M|--until UTC] [--stage A<k>..]
 taskq steer N --text T | taskq failed N --text T
 taskq close N [M..] [--text VERDICT] | taskq gate N | taskq approve N [--sha SHA]
 taskq recipe NAME --run ID [--goal G] [--start]
 taskq status [--project P..] [--only L,..] [--json] [--timing] | taskq status --closed [--page N] [--since T] [--task N [--transcript [--before B]]] | taskq inbox N [--json]
 taskq issue N | taskq issue N --comment T|--label L [--op-id ID] [--json]
 taskq claim-issue [N] [--json]
 taskq deployed --version V --env E --range A..B [--host H] [--json]
 taskq results [N] [--action open|read|accept|followup|reopen|later|drop|make] [--sha SHA] [--text REASON] [--until UTC] [--page N|--before TIME:N] --json
 taskq profile [guide|export|tasks|board|actions|watch|health] [--since 24h] | taskq retro
 taskq pm [--every 3m] [--project P..] | taskq pm stop
 taskq wait [--window MIN] [--only L,..]
 taskq cleanup [--force] [--dry-run] [--all] | taskq doctor [--dry-run] [--hosts [--fresh]] [--json] | taskq update [--check]
 taskq enroll <https-url> [--headless] [--telemetry] | taskq connection status | taskq disconnect
 taskq service install|run|pause|resume|status|restart|stop|uninstall|list
 taskq service setup [--runtime claude|codex] [--client C] [--target PATH..] [--json]
 taskq service onboard
 taskq service dev-build (macOS channel dev)
 taskq service app-server --action prepare|launch|observe|cancel|reconcile --request JSON
 taskq service slots --identity PID | --action reserve|observe|release --request JSON [--json]
 taskq service stop [--mode drain|full] (macOS)
 taskq service focus [all|off|<label..>] [--request TEXT]
 taskq balance [claude=N codex=N antigravity=N | --clear]
 taskq service capacity [--host H] [--max N|none] [--claude N] [--codex N] [--antigravity N] [--codex-app N] [--load-gate on|off] [--cpu-max P] [--memory-min P] [--claim any|spillover|labelled|none] [--default-runtime claude|codex|antigravity]
 taskq service permissions [--runtime R --mode restricted|workspace|full|none [--scope host|project]]
 taskq service resources [--grant ID --actor ID --hours N [--project P].. [--runtime R].. [--host-policy] [--project-limits] | --revoke ID]
 taskq service pm [--grant ID --actor ID --hours N --project P.. --runtime R.. | --revoke ID] [--json]
 taskq tray [--open | --snapshot DIR [--route R,..]] | taskq selftest | taskq --version | taskq help [--json]
Writes to a task take [--op-id ID] [--revision R] [--json] (taskq.md section 5).
Text templates and limits: taskq.md sections 1, 2, 5, 7.";

/// #1910: the one PM command catalog; `taskq help` renders it, `taskq help --json` prints it for other PM surfaces.
pub(crate) const PM_COMMANDS: &str = include_str!("pm-commands.json");

/// `taskq pm`: the exact manager instructions any agent session follows; TaskQ owns them, the session only runs them.
pub(crate) fn pm_instructions(a: &Args) -> Res<String> {
    const USE: &str = "usage: taskq pm [--every 3m] [--project P..] | taskq pm stop";
    if a.flags.keys().any(|k| k != "every" && k != "project") || a.all("every").len() > 1 { return Err(USE.into()); }
    match &a.pos[1..] {
        [stop] if stop == "stop" && a.flags.is_empty() => return Ok(include_str!("pm-manager-stop.md").into()),
        [] => {}
        _ => return Err(USE.into()),
    }
    let every = a.one("every").unwrap_or("3m");
    let factor = match every.chars().last() { Some('m') => 60, Some('h') => 3600, _ => return Err(USE.into()) };
    let seconds = every[..every.len() - 1].parse::<u32>().ok().and_then(|n| n.checked_mul(factor)).filter(|s| (60..=86400).contains(s)).ok_or("taskq pm: --every must be 1m..24h, like 3m or 1h")?;
    let projects = a.all("project");
    if projects.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))) { return Err(USE.into()); }
    let (scope, list, cmd) = if projects.is_empty() {
        ("all".to_string(), "every project of this surface (PilotQ: `pilotq projects list`, each with its root; elsewhere the one `taskq status` that covers them all).".to_string(), format!("taskq pm --every {every}"))
    } else {
        let names = projects.iter().map(|p| format!("--project {p}")).collect::<Vec<_>>().join(" ");
        (projects.join(", "), format!("{} (PilotQ: roots from `pilotq projects list`; elsewhere `taskq status {names}`).", projects.join(", ")), format!("taskq pm --every {every} {names}"))
    };
    Ok(include_str!("pm-manager.md").replace("{{every}}", every).replace("{{seconds}}", &seconds.to_string()).replace("{{scope}}", &scope).replace("{{list}}", &list).replace("{{cmd}}", &cmd))
}

pub(crate) fn help(json: bool) -> Res<String> {
    if json { return Ok(PM_COMMANDS.trim_end().into()); }
    let catalog: Value = serde_json::from_str(PM_COMMANDS).map_err(|e| format!("PM command catalog: {e}"))?;
    let mut out = format!("{USAGE}

PM commands (catalog: taskq help --json):
");
    for c in catalog["commands"].as_array().into_iter().flatten() {
        out += &format!("  {}
      {}
", c["use"].as_str().unwrap_or_default(), c["purpose"].as_str().unwrap_or_default());
    }
    out += "PM procedures:
";
    for p in catalog["procedures"].as_array().into_iter().flatten() {
        out += &format!("  '{}': {}; procedure: {}
", p["trigger"].as_str().unwrap_or_default(), p["command"].as_str().unwrap_or_default(), p["doc"].as_str().unwrap_or_default());
    }
    Ok(out.trim_end().into())
}

#[derive(Default)]
pub(crate) struct Args {
    pub(crate) pos: Vec<String>,
    pub(crate) flags: HashMap<String, Vec<String>>,
}

impl Args {
    pub(crate) fn parse(v: &[String]) -> Res<Args> {
        let mut a = Args::default();
        let mut it = v.iter();
        while let Some(x) = it.next() {
            match x.strip_prefix("--") {
                Some(k) => {
                    let val = if matches!(k, "text-stdin" | "later" | "hold" | "start" | "check" | "force" | "help" | "json" | "apply" | "headless" | "telemetry" | "host-policy" | "project-limits" | "closed" | "open" | "transcript" | "dry-run" | "all" | "clear" | "timing" | "gate") || matches!(k,"hosts"|"fresh")&&a.pos==["doctor"] || k=="host"&&a.pos==["service","uninstall"] { String::new() } else { it.next().cloned().ok_or(format!("--{k} needs a value"))? };
                    a.flags.entry(k.to_string()).or_default().push(val);
                }
                None => a.pos.push(x.clone()),
            }
        }
        let used = match a.pos.first().map(String::as_str) {
            Some("close" | "start" | "balance" | "pm") => a.pos.len(),
            Some("answer") if a.pos.get(1).is_some_and(|p| p.contains('.')) => a.pos.len(),
            Some("service") if a.pos.get(1).is_some_and(|p| p == "focus") => a.pos.len(),
            Some("service") if a.pos.get(1).is_some_and(|p| p == "channel") => 3,
            Some("rules") => 6,
            Some("accounts") => 5,
            Some("unpin" | "deps" | "service" | "profile" | "add" | "finding" | "approve" | "gate" | "recipe" | "ask" | "answer" | "pick" | "result" | "requeue" | "later" | "failed" | "steer" | "inbox" | "issue" | "claim-issue" | "results" | "enroll" | "connection") => 2,
            _ => 1,
        };
        if a.pos.len() > used {
            return Err(format!("unused positional arguments: {}", a.pos[used..].join(" ")));
        }
        if a.flags.contains_key("text-stdin") || a.flags.contains_key("text-file") {
            if ["text", "text-stdin", "text-file"].iter().filter(|k| a.flags.contains_key(**k)).count() != 1
                || !matches!(a.pos.first().map(String::as_str), Some("answer" | "steer" | "requeue")) || a.pos.get(1).is_some_and(|n| n.contains('.')) {
                return Err("use one text source (--text, --text-stdin or --text-file) for answer, steer or requeue without a numbered choice".into());
            }
        }
        if a.flags.contains_key("json-file") && (!a.flags.contains_key("json") || a.flags.contains_key("closed")
            || !matches!(a.pos.first().map(String::as_str), Some("status" | "answer" | "steer" | "requeue" | "later" | "close" | "start" | "failed"))) {
            return Err("--json-file requires --json with status or a task control/reply command (not --closed)".into());
        }
        let worker_files = a.flags.contains_key("file") && !a.flags.contains_key("files-from") && matches!(a.pos.first().map(String::as_str), Some("result" | "ask"));
        if (a.flags.contains_key("file")||a.flags.contains_key("files-from")) && !worker_files && (!matches!(a.pos.first().map(String::as_str),Some("answer"|"steer"|"requeue"))||a.pos.get(1).is_some_and(|n|n.contains('.'))) {
            return Err("--file requires answer N, steer N, owner requeue N, result N or ask N; choose a text reply instead of a numbered option".into());
        }
        if a.all("files-from").len()>1{return Err("pass --files-from once with a JSON array of file paths".into());}
        Ok(a)
    }
    pub(crate) fn read_text(&mut self, input: &mut dyn std::io::Read) -> Res<()> {
        if let Some(paths)=self.flags.remove("files-from") {
            let data=fs::read(&paths[0]).map_err(|e|format!("Cannot read attachment list: {e}; supply a readable JSON file"))?;
            let files:Vec<String>=serde_json::from_slice(&data).map_err(|e|format!("Attachment list: {e}; use a JSON array of file paths"))?;
            self.flags.entry("file".into()).or_default().extend(files);
        }
        if self.flags.remove("text-stdin").is_some() {
            let mut text = String::new();
            input.read_to_string(&mut text).map_err(|e| format!("cannot read UTF-8 reply from stdin: {e}"))?;
            self.flags.insert("text".into(), vec![text]);
        } else if let Some(path) = self.flags.remove("text-file") {
            let text = fs::read_to_string(path.last().ok_or("--text-file needs a path")?).map_err(|e| format!("cannot read UTF-8 reply file: {e}"))?;
            self.flags.insert("text".into(), vec![text]);
        }
        Ok(())
    }
    pub(crate) fn one(&self, k: &str) -> Option<&str> {
        self.flags.get(k).and_then(|v| v.last()).map(String::as_str)
    }
    pub(crate) fn need(&self, k: &str) -> Res<&str> {
        self.one(k).filter(|s| !s.trim().is_empty()).ok_or(format!("--{k} is required"))
    }
    pub(crate) fn all(&self, k: &str) -> Vec<String> {
        self.flags.get(k).cloned().unwrap_or_default()
    }
    pub(crate) fn num(&self, i: usize) -> Res<u64> {
        let s = self.pos.get(i).ok_or("task number missing")?;
        s.trim_start_matches('#').parse().map_err(|_| format!("not a task number: {s}"))
    }
}
