//! The `taskq` command line (taskq.md § 5). Every write is one `taskq:` comment that `fold` accepts.

/// One service log line (§ 4): stamped and folded in `service run`, plain on the CLI.
macro_rules! log {
    ($($t:tt)*) => { $crate::platform::log(&format!($($t)*)) };
}

mod admission;
mod draft_deps;
mod check;
mod cli;
mod config;
#[cfg(any(target_os="macos",windows,test))]
mod agentq;
mod host;
mod host_control;
mod external_slots;
mod board;
mod core;
mod control;
mod data;
mod doctor;
mod health;
mod http;
mod deploy;
mod dispatch;
mod display;
mod enrollment;
mod incident;
mod incoming;
mod labels;
mod balance;
mod link;
mod recipe;
mod requests;
mod reply_files;
mod platform;
mod service_versions;
mod readiness;
mod policy;
mod resources;
mod paths;
mod pm;
mod lifecycle;
mod recovery;
#[cfg(target_os = "linux")]
mod linux_sandbox;
#[cfg(windows)]
mod windows_sandbox;
mod profile;
mod telemetry;
mod telemetry_sync;
mod time;
mod tidy;
mod tools;
mod runtime;
mod service;
mod service_run;
mod wake;
mod setup;
mod dialogue;
mod transcript;
mod messages;
mod conversations;
mod snapshot;
mod reports;
mod summary;
#[cfg(any(target_os = "macos", windows))]
mod icon;
#[cfg(any(target_os = "macos", windows))]
mod notify;
#[cfg(any(target_os = "macos", windows))]
mod tray;
#[cfg(any(target_os = "macos", windows))]
mod ui_bundle;
#[cfg(any(target_os = "macos", windows, test))]
mod pending;

use crate::cli::{Args, help, pm_instructions};
use crate::cli::connection::{connection_status, enroll_cmd};
use crate::cli::handin::{ask_cmd, later_cmd, result_cmd};
use crate::cli::host_cmd::{accounts_cli, balance_cli, doctor_cmd};
use crate::cli::inbox::{inbox, inbox_json, wait};
#[cfg(target_os = "macos")]
use crate::cli::service_cmd::bundled_tray;
use crate::cli::status_cmd::{cleanup_lines, status_all, status_cmd};
use crate::cli::task_cmd::{add_cmd, claim_issue_cmd, deployed_cmd, gate_task, issue_cmd, reroute, run_recipe, start_tasks};
use crate::cli::text::{close_check, formats_selftest, media_links, task_text};
use crate::config::{Ctx, ctx, load};
use crate::dispatch::{Op, Receipt, answer_picks, close_checked};
use crate::paths::{find_root, needs_checkout, project_name};
use std::path::Path;
use std::time::Duration;

pub type Res<T> = Result<T, String>;

/// § 5 receipts: JSON lines with `--json`, else the text lines; any receipt not accepted fails after all are shown.
fn emit(rs: &[Receipt], json: bool) -> Res<()> {
    for r in rs {
        match (json, r.ok()) {
            (true, _) => println!("{}", r.json()),
            (false, true) => println!("{}", r.text()),
            (false, false) => eprintln!("{}", r.text()),
        }
    }
    if rs.iter().all(Receipt::ok) { Ok(()) } else { Err(String::new()) } // already shown: exit 1 quietly
}

fn json_output(text: &str, file: Option<&str>) -> Res<()> {
    if let Some(path) = file { platform::write_atomic(Path::new(path), text).map_err(|e| format!("JSON file write failed: {e}")) }
    else { println!("{text}"); Ok(()) }
}

fn golden_selftest(traces: &str, published: bool) -> Res<usize> {
    let result = core::golden(traces);
    if published && result.is_err() {
        incident::detect("invariant.golden", concat!("src/main.rs:", line!()), "none", &["fold.json"], &["golden:mismatch"]);
    }
    result
}

#[tracing::instrument(name="run",skip_all)]
fn run(argv: &[String]) -> Res<()> {
    let mut a = Args::parse(argv)?;
    telemetry::diagnostic::poll(&platform::default_install());
    if let Some(selector)=a.one("diagnose") {
        let milliseconds=a.one("diagnose-ms").unwrap_or("60000").parse::<u64>().map_err(|_|"invalid --diagnose-ms")?;
        let limit=a.one("diagnose-limit").unwrap_or("256").parse::<u64>().map_err(|_|"invalid --diagnose-limit")?;
        telemetry::diagnostic::select(selector,Duration::from_millis(milliseconds),limit)?;
    } else if a.one("diagnose-ms").is_some() || a.one("diagnose-limit").is_some() {return Err("diagnostic limits require --diagnose".into());}
    if a.pos.is_empty() || a.pos.first().is_some_and(|s| s == "help") || a.pos.iter().any(|s| s == "-h") || a.flags.contains_key("help") {
        println!("{}", help(a.flags.contains_key("json"))?);
        return Ok(());
    }
    a.read_text(&mut std::io::stdin().lock())?;
    let cmd = a.pos.first().map(String::as_str).unwrap_or("");
    if std::env::var_os("TASKQ_ROOT").is_some() {
        find_root()?; // § 5: a stale TASKQ_ROOT refuses every command, not only those that read the project
    }
    match cmd {
        "profile" => match a.pos.get(1) {
            Some(v) if v=="diagnose" => {
                let pid=a.need("pid")?.parse().map_err(|_|"invalid --pid")?;
                let period=a.one("duration-ms").unwrap_or("60000").parse().map_err(|_|"invalid --duration-ms")?;
                let limit=a.one("limit").unwrap_or("256").parse().map_err(|_|"invalid --limit")?;
                let request=telemetry::diagnostic::request(&platform::default_install(),pid,a.need("selector")?,period,limit)?;
                println!("Diagnostic selection requested: {request}; verify target status/export; no application acknowledgment yet");Ok(())
            }
            None if a.one("since").is_some() => Err("--since needs a view: taskq profile export|tasks|board --since 24h".into()),
            None => { print!("{}", profile::report(&platform::default_install())); Ok(()) }
            Some(v) => { print!("{}", profile::view(&platform::default_install(), v, a.one("since"))?); Ok(()) }
        },
        "pm" => { print!("{}", pm_instructions(&a)?); Ok(()) }
        "rules" => match &a.pos[1..] {
            [phase, root, role, task, attempt] if phase == "compact" => {
                let n = |s: &String| s.parse::<u64>().map_err(|_| format!("rules compact: {s} is not a number"));
                print!("{}", runtime::compact(Path::new(root), role, n(task)?, n(attempt)?).map_err(|e| e.to_string())?);
                Ok(())
            }
            // a worker or reviewer under the project hook: its rules come with its turn (§ 8)
            [pm, _] if pm == "pm" && std::env::var_os("TASKQ_ROLE").is_some() => Ok(()),
            [pm, phase] if pm == "pm" && (phase == "start" || phase == "compact") => {
                print!("{}", runtime::pm(&find_root()?, phase).map_err(|e| e.to_string())?);
                Ok(())
            }
            // #1946: project CI runs this so a contract edit over the cap fails before merge, not at worker start
            [size] if size == "size" => {
                let (len, cap) = runtime::rules_size(&find_root()?).map_err(|e| e.to_string())?;
                println!("Rules: {len} of {cap} bytes, headroom {}", cap - len);
                Ok(())
            }
            _ => Err("usage: taskq rules size | taskq rules compact ROOT ROLE TASK ATTEMPT | taskq rules pm start|compact".into()),
        },
        "selftest" => {
            let n = golden_selftest(core::GOLDEN, !platform::VERSION.contains('-')).map_err(|e| format!("fold: FAIL {e}"))?;
            println!("fold: {n} golden traces PASS");
            println!("{}", formats_selftest()?);
            if std::env::var_os("TASKQ_SELFTEST_OFFLINE").is_some() {
                println!("board: unknown (TASKQ_SELFTEST_OFFLINE set; no board read)");
            } else if let Ok(root) = find_root() {
                let c = ctx()?;
                let (views, hosts) = service::views(c.board.as_ref(), &mut Default::default(), &mut Default::default(), c.cfg.ttl)?;
                let now = c.board.now()?;
                let live = views.iter().filter_map(|v| {
                    let a = v.projection.task.attempt?;
                    let h = service::host_of(&v.projection.task)?;
                    (hosts.iter().any(|hb| hb.title == format!("taskq host {h}")
                        && now - hb.updated <= c.cfg.ttl && hb.body.lines().any(|l| l == format!("#{} {a}", v.issue.number))))
                        .then(|| (c.cfg.repo.clone(), a))
                }).collect();
                println!("{}", profile::selftest(&c.cfg.install, &c.cfg.repo, &live, c.cfg.ttl.max(0) as u64)?);
                let open = views.len();
                println!("board: {} as {} on {}", c.cfg.repo, c.board.me()?, c.cfg.web_host);
                // #1935: a preset the repository's plan cannot finish, with the owner's fixes
                println!("delivery: {} r{}: {}", c.cfg.delivery, c.cfg.delivery_revision, match c.board.delivery_gap(c.cfg.delivery.mode()) {
                    Ok(None) => "no plan mismatch found".into(),
                    Ok(Some(gap)) => format!("MISMATCH {gap}; fix: make the repository public (merge queue), upgrade the plan, set delivery.preset direct, or merge each PR by hand"),
                    Err(e) => format!("unknown ({e})"),
                });
                println!("open issues: {open}");
                println!("host: {}", c.cfg.me);
                println!("root: `{}`", root.display());
            } else {
                println!("board: unknown (no project config found; no board read)");
            }
            let capture=coreq::telemetry::capture::qualify().map_err(|e|format!("telemetry: FAIL {e}"))?;
            // #1419: entry.rs gave the offline (staged release) selftest a private store.
            if std::env::var_os("TASKQ_SELFTEST_OFFLINE").is_some() {println!("telemetry: staged capture committed {} observations (private store; running writer not used)",capture["committed"]);}
            else {println!("telemetry: local capture committed {} observations",capture["committed"]);}
            // #1856: the release staging selftest is offline; a host gap must not hold the update that may fix it
            // the dev install and tray Add project check the build and the project, not this host's tools (TASKQ_SELFTEST_TOOLS=off)
            for var in ["TASKQ_SELFTEST_OFFLINE","TASKQ_SELFTEST_TOOLS"] {
                if std::env::var_os(var).is_some() {println!("tools: unknown ({var} set; no tool probe)");return Ok(());}
            }
            let rows=tools::rows(&platform::default_install(),find_root().ok().as_deref());
            for (line,failed) in &rows {println!("tool {}{line}",if *failed {"FAIL "} else {""});}
            let failed:Vec<_>=rows.iter().filter(|r|r.1).map(|r|r.0.split(':').next().unwrap_or("?")).collect();
            if failed.is_empty() {Ok(())} else {Err(format!("tools: FAIL {}; fix steps above (docs/design/host-service.md#host-tools)",failed.join(", ")))}
        }
        "update" => {
            let install = platform::default_install();
            let check = a.flags.contains_key("check");
            println!("{}", incident::disclosure(find_root().ok().as_deref()));
            match platform::update(&install, check)? {
                Some(v) if check => println!("update available: {} -> {v}", platform::tool(&install).current().unwrap_or_else(|| platform::VERSION.into())),
                Some(v) => println!("updated to {v}; the service restarts on its next update check"),
                None => println!("up to date ({})", platform::tool(&install).current().unwrap_or_else(|| platform::VERSION.into())),
            }
            if !check {for l in tools::gaps(&install, find_root().ok().as_deref()) {println!("{l}");}}
            Ok(())
        }
        // § 5: read-only probes on this host, no board
        "agents" => {
            for r in runtime::agents(&platform::default_install()).as_array().into_iter().flatten() { println!("{}", runtime::agents_line(r)); }
            Ok(())
        }
        // § 3 Runtime accounts: list reads every host; enable, disable, move and add write this host only
        "accounts" => accounts_cli(&a),
        "balance" => balance_cli(&a),
        "service" => cli::service_cmd::service(&a),
        // #2060 (docs/design/worktrees.md#commands): the tidy of each registered project of this host
        "cleanup" if a.flags.contains_key("all") => {
            let roots = host::projects(&platform::default_install())?;
            if roots.is_empty() { println!("cleanup: no registered project on this host"); }
            for root in roots {
                println!("{}: {}", project_name(&root), root.display().to_string().trim_start_matches(r"\\?\"));
                let lines = needs_checkout(&root, "cleanup").and_then(|_| load(&root)).and_then(|(cfg, raw)| {
                    let b = cfg.board.open(&cfg.web_host, &cfg.repo, raw["label_schema"].as_str(), &platform::tool(&cfg.install))?;
                    Ok(cleanup_lines(&*b, &cfg, &a))
                });
                match lines {
                    Ok(lines) => for l in lines { println!("{l}"); },
                    Err(e) => println!("skipped: {e}"),
                }
            }
            Ok(())
        }
        // #2060 (docs/design/host-service.md#host-doctor): one host doctor pass now; no board
        "doctor" => doctor_cmd(&a),
        // § 5: an owner session (§ 4) in this project; no board
        "retro" => {
            let (cfg, _) = load(&find_root()?)?;
            let r = setup::open(&cfg.install, &cfg.root, setup::RETRO)?;
            println!("retro: request {} pending; the host service opens a new session in {}", r.id, cfg.root.display());
            Ok(())
        }
        "tray" => cli::service_cmd::tray(platform::default_install(), a.one("snapshot"), a.one("open").is_some(), &a.all("route").iter().flat_map(|r| r.split(',')).map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).collect::<Vec<_>>(), a.one("icon")),
        "" => Err("command missing; use `taskq help`".into()),
        "status" if !a.flags.contains_key("closed") => status_all(&a),
        "results" => match reports::read_cli(&find_root()?,&a)? {Some(value)=>{println!("{value}");Ok(())},None=>command(&ctx()?,cmd,&a)},
        _ => command(&ctx()?, cmd, &a),
    }
}

/// Writes one command's receipts: to `--json-file` when given, else through `emit`.
pub(crate) fn respond(a: &Args, rs: &[Receipt], json: bool) -> Res<()> {
    if let Some(path) = a.one("json-file") {
        json_output(&rs.iter().map(Receipt::json).collect::<Vec<_>>().join("\n"), Some(path))?;
        if rs.iter().all(Receipt::ok) { Ok(()) } else { Err(String::new()) }
    } else { emit(rs, json) }
}

/// Keep a present empty dependency list: it is the explicit clear operation (§1).
fn deps_command(b:&dyn board::Board,root:&Path,a:&Args,op:&Op,authorized:bool)->Res<Receipt> {
    for (flag,values) in &a.flags {
        if !["board","deps","op-id","revision","json"].contains(&flag.as_str()) || values.len()!=1 {return Err(format!("deps: unsupported or repeated --{flag}"));}
    }
    Ok(draft_deps::amend(b,root,a.num(1)?,a.need("board")?,a.one("deps").ok_or("--deps is required")?,op,authorized))
}

fn command(c: &Ctx, cmd: &str, a: &Args) -> Res<()> {
    let (b, cfg, root) = (c.board.as_ref(), &c.cfg, c.cfg.root.as_path());
    let text = a.one("text").unwrap_or("");
    if matches!(cmd, "later" | "failed" | "steer") || cmd == "requeue" && dispatch::agent() || (cmd == "answer" && !a.pos.get(1).is_some_and(|p| p.contains('.'))) {
        if a.all("file").is_empty() || !text.trim().is_empty() { task_text(cmd, text)?; }
    }
    if cmd == "add" && a.flags.contains_key("later") { task_text("later", text)?; }
    if matches!(cmd, "ask" | "result" | "requeue" | "later" | "failed" | "steer" | "answer" | "pick") {
        let given = [a.all("option"), a.all("checks"), a.all("link")].concat();
        media_links(&[vec![text], given.iter().map(String::as_str).collect()].concat())?;
    }
    let revision = a.one("revision").map(|r| r.parse::<u64>().map_err(|_| format!("bad --revision {r}"))).transpose()?;
    let mut op = Op { id: a.one("op-id").map(String::from), revision };
    if op.id.as_deref().is_some_and(|i| i.is_empty() || i.contains(char::is_whitespace)) {
        return Err("--op-id: one word".into());
    }
    let json = a.flags.contains_key("json");
    let emit = |rs: &[Receipt], json: bool| respond(a, rs, json);
    if revision.is_some() && (cmd == "close" && a.pos.len() > 2 || cmd == "answer" && a.pos.len() > 2) {
        return Err("--revision names one task".into());
    }
    match cmd {
        "results" => {
            let value=match reports::read_cli(root,a)? {
                Some(value)=>value,
                None=>reports::act_with_reason(b,root,&c.raw,a.num(1)?,a.need("sha")?,a.one("action").unwrap_or("open"),a.one("text").unwrap_or(""),a.one("until"))?
            };
            println!("{value}"); Ok(())
        }
        "unpin" => {
            if a.flags.iter().any(|(k,v)| !["host","revision","op-id","json"].contains(&k.as_str()) || v.len()!=1) {return Err("unpin accepts --host, --revision, --op-id and --json once each".into());}
            emit(&[dispatch::unpin(b,a.num(1)?,a.need("host")?,&op,dispatch::agent()).wake(root)],json)
        }
        "reroute" => {
            if dispatch::agent() { return Err("only the owner or PM can reroute strict task pins".into()); }
            emit(&reroute(b, root, a.need("from")?, a.need("to")?)?, json)
        }
        "labels" => {
            let mut names = labels::FIXED.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            names.extend(b.issues(None)?.into_iter().flat_map(|i| i.labels));
            b.ensure_labels(&names)?;
            println!("Label catalog verified for {}", project_name(root));
            Ok(())
        }
        "add" | "finding" => add_cmd(c, cmd, a, &mut op),
        "deps" => {
            let authorized=["TASKQ_ATTEMPT","TASKQ_TASK","TASKQ_ROLE","TASKQ_SERVICE"].iter().all(|key|std::env::var_os(key).is_none());
            emit(&[deps_command(b,root,a,&op,authorized)?],json)
        }
        "gate" => emit(&[gate_task(b, root, a.num(1)?, &op)], json),
        "start" => emit(&start_tasks(b, root, &a.pos[1..], &op)?, json),
        "approve" => emit(&[dispatch::approve(b, root, a.num(1)?, a.one("sha"), &op, dispatch::agent())], json),
        "recipe" => {
            let name = a.pos.get(1).ok_or("recipe name missing")?;
            emit(&run_recipe(b, root, &c.raw, name, a.need("run")?, a.one("goal"), a.flags.contains_key("start"))?, json)
        }
        "ask" => ask_cmd(c, a, &op),
        "answer" if a.pos.get(1).is_some_and(|p| p.contains('.')) => {
            emit(&answer_picks(b, cfg, &a.pos[1..], &op)?, json)
        }
        "pick" => emit(&[dispatch::pm_pick(b, root, a.pos.get(1).ok_or("N.K missing")?, text, &op)?], json),
        "answer" => {
            if a.all("file").is_empty(){emit(&dispatch::answer_text(b, cfg, a.num(1)?, a.need("text")?, &op)?, json)}
            else{emit(&[dispatch::act_files(b,root,a.num(1)?,cmd,text,&op,&a.all("file"))],json)}
        }
        "result" => result_cmd(c, a, &op),
        "requeue" => {
            let n = a.num(1)?;
            emit(&[dispatch::act_files(b,root,n,cmd,text,&op,&a.all("file"))],json)
        }
        "later" => later_cmd(c, a, &op),
        "steer" | "failed" => {
            let n = a.num(1)?;
            emit(&[dispatch::act_files(b,root,n,cmd,if a.all("file").is_empty(){a.need("text")?}else{text},&op,&a.all("file"))],json)
        }
        "close" => {
            let mut out = vec![];
            for i in 1..a.pos.len() {
                let n = a.num(i)?;
                out.push(close_checked(b, cfg, n, text, &op, &|issue, t| close_check(text, issue, t, b.defer_publication(cfg.delivery.mode()))));
            }
            emit(&out, json)
        }
        // docs/design/enrollment.md: observation only, no task answers or commands; the board stays the truth
        "enroll" => enroll_cmd(c, a),
        "connection" if a.pos.get(1).is_some_and(|p| p == "status") => connection_status(c),
        "disconnect" => {
            println!("{}", enrollment::disconnect(&cfg.install)?.as_deref().unwrap_or("connection: none"));
            Ok(())
        }
        "status" => status_cmd(c, a),
        "cleanup" => {
            needs_checkout(root, "cleanup")?;
            for l in cleanup_lines(b, cfg, a) { println!("{l}"); }
            Ok(())
        }
        "issue" => issue_cmd(c, a, &op),
        "deployed" => deployed_cmd(c, a),
        "claim-issue" => claim_issue_cmd(c, a),
        "inbox" => {
            if a.flags.contains_key("json") { println!("{}", inbox_json(b, root, &cfg.me, a.num(1)?)?); } else { println!("{}", inbox(b, root, a.num(1)?)?); }
            Ok(())
        }
        "wait" => {
            let w = a.one("window").map(|w| w.parse().map_err(|_| "bad --window")).transpose()?.unwrap_or(10);
            for l in wait(b, cfg, w, &a.all("only").iter().flat_map(|x| x.split(',')).map(String::from).collect::<Vec<_>>())? {
                println!("{l}");
            }
            Ok(())
        }
        _ => Err(format!("unknown command `{cmd}`; use `taskq help`")),
    }
}

#[tracing::instrument(name="application",skip_all)]
fn application() {
    #[cfg(windows)]
    if std::env::args().nth(1).as_deref() == Some("__windows-security-restore") {
        if let Err(e) = windows_sandbox::restore(&std::env::args().skip(2).collect::<Vec<_>>()) { eprintln!("{e}"); crate::platform::exit(1); }
        return;
    }
    #[cfg(windows)]
    if std::env::args().nth(1).as_deref() == Some("__windows-codex") {
        if let Err(e) = windows_sandbox::run(&std::env::args().skip(2).collect::<Vec<_>>()) { eprintln!("{e}"); crate::platform::exit(1); }
        return;
    }
    #[cfg(target_os = "linux")]
    if std::env::current_exe().ok().is_some_and(|p| p == Path::new(linux_sandbox::HELPER)) {
        match linux_sandbox::run(&std::env::args().skip(1).collect::<Vec<_>>()) {
            Ok(code) => crate::platform::exit(code),
            Err(e) => { eprintln!("{e}"); crate::platform::exit(1); }
        }
    }
    incident::install_panic_hook();
    #[allow(unused_mut)]
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `<install>/taskq` is the launcher: it runs the current release (coreq.md § Launcher).
    let install = platform::default_install();
    let exe = std::env::current_exe().ok().and_then(|e| e.canonicalize().ok());
    let t = platform::tool(&install);
    // #2049 A1: Windows tasks start the GUI launcher `<install>/taskqw.exe`; it runs every command windowless.
    let gui = exe.as_deref().and_then(Path::file_name).is_some_and(|f| f.eq_ignore_ascii_case(t.gui_exe_name()));
    if exe.as_deref().is_some_and(|e| t.is_launcher(e)) && t.current().is_some() {
        if gui { coreq::launcher::launch(&t, &args); }
        // #1566: CoreQ's Windows launcher ends a CLI call through `ExitProcess`, the TLS-teardown hang `platform::exit` avoids.
        // M15: the launcher is the Windows app. No arguments on its own console (Explorer, Start menu, taskbar pin) opens
        // TaskQ without a console window; the tray runs in this process, so its exe path, and the pins keyed by it,
        // stay the same across updates. ponytail: a launcher an update could not rewrite runs its older tray until the
        // tray's version check ends it.
        #[cfg(windows)]
        if args.is_empty() && platform::own_console() { args = vec!["tray".into(), "--open".into()]; }
        #[cfg(windows)]
        if args.first().map(String::as_str) != Some("tray") && !(args.first().map(String::as_str) == Some("service") && args.get(1).map(String::as_str) == Some("run")) {
            let v = coreq::launcher::pick(&t, false).unwrap_or_else(|e| platform::die(&e));
            let status = platform::console_command(t.release_exe(&v)).args(&args).env(t.env("INSTALL"), &t.install).status();
            platform::exit(status.map_or_else(|e| platform::die(&format!("start {v}: {e}")), |s| s.code().unwrap_or(1)));
        }
        #[cfg(not(windows))]
        coreq::launcher::launch(&t, &args);
        #[cfg(windows)]
        if args.first().map(String::as_str) != Some("tray") { coreq::launcher::launch(&t, &args); }
    }
    #[cfg(target_os = "macos")]
    if args.is_empty()&&exe.as_deref().is_some_and(|exe|bundled_tray(exe,&install)) {
        profile::init(&install);
        if let Err(e) = cli::service_cmd::tray(install, None, false, &[], None) {
            platform::die(&e);
        }
        return;
    }
    profile::init(&install);
    let r = run(&args);
    // #1351: the command already took effect; hand queued receipts to the writer before the exit flush, which has the same bound.
    telemetry::settle(Duration::from_secs(10));
    // § 4 budget: this process's board requests by source, for `taskq profile`
    requests::flush(&requests::source(&args, std::env::var("TASKQ_ROLE").ok().as_deref()));
    if let Err(e) = r {
        if e.is_empty() {
            crate::platform::exit(1);
        }
        platform::die(&e);
    }
}


#[cfg(test)]
#[path = "main_tests.rs"]
pub(crate) mod tests;
#[cfg(test)]
#[path = "serde_shape_tests.rs"]
mod serde_shape_tests;
