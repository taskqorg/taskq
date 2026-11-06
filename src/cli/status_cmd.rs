//! The `status` report (§ 5).

use crate::board::Board;
use crate::cli::Args;
use crate::cli::inbox::{closed_history, closed_transcript};
use crate::cli::text::compact;
use crate::config::{Ctx, ctx, load};
use crate::paths::{PROJECT_ROOT, dot, find_root, project_name};
use crate::service::Config;
use crate::{Res, command, json_output};
use crate::{
    check, conversations, data, deploy, host, platform, reports, requests, service, snapshot, tidy,
};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

// R10 A1: each status block's live hosts and their served-project counts, for the installation's anomaly rows
thread_local!(pub(crate) static SERVED: std::cell::RefCell<Vec<Vec<(String, usize)>>> = const { std::cell::RefCell::new(vec![]) });
/// `taskq cleanup [--force] [--dry-run]` of one root ([worktrees](docs/design/worktrees.md#commands)): every removal,
/// refusal, kept task worktree and root move with its reason, else `nothing to clean`.
pub(crate) fn cleanup_lines(b: &dyn Board, cfg: &Config, a: &Args) -> Vec<String> {
    // a child the local service still runs keeps its worktree, whatever the board says
    let st: Value = fs::read_to_string(dot(&cfg.root).join("status.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let live: Vec<u64> = st["kids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|k| k["task"].as_u64())
        .collect();
    let t = tidy::Tidy {
        board: b,
        root: &cfg.root,
        me: &cfg.me,
        boot: platform::boot_time,
        external: cfg.external,
        force: a.flags.contains_key("force"),
        dry: a.flags.contains_key("dry-run"),
    };
    let lines = t.run(&mut |n| live.contains(&n), &|_| None).lines;
    if lines.is_empty() {
        vec!["nothing to clean".into()]
    } else {
        lines
    }
}

/// R6 snapshot of one project: the board read, with ready rows the board alone leaves undecided taking this host's
/// live admission from `status.json` (A3), and #1979 Decide from the collected reports (no board call).
/// #2291: one comment read per open task dominates `status`; 8 board clients read them in parallel. Any failure
/// leaves the task out of the cache, so the snapshot read fetches it as before.
pub(crate) fn prefetch_comments(c: &Ctx) -> service::CommentCache {
    let Ok(issues) = c.board.issues(None) else {
        return Default::default();
    };
    let now = c.board.now().unwrap_or(0);
    let todo: Vec<(u64, i64)> = issues
        .iter()
        .filter(|i| {
            i.trusted && !service::is_host(i) && (service::is_task(i) || service::is_held(i))
        })
        .map(|i| (i.number, i.updated))
        .collect();
    if todo.is_empty() {
        return Default::default();
    }
    let (cfg, schema) = (&c.cfg, c.raw["label_schema"].as_str());
    let prefer = crate::board::local::preferred();
    std::thread::scope(|s| {
        let handles: Vec<_> = todo
            .chunks(todo.len().div_ceil(8))
            .map(|part| {
                s.spawn(move || {
                    crate::board::local::prefer(prefer);
                    let Ok(b) = cfg.board.open(
                        &cfg.web_host,
                        &cfg.repo,
                        schema,
                        &platform::tool(&cfg.install),
                    ) else {
                        return vec![];
                    };
                    part.iter()
                        .filter_map(|&(n, updated)| {
                            b.comments(n).ok().map(|cs| (n, (updated, now, cs)))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    })
}

pub(crate) fn status_snapshot(c: &Ctx, a: &Args) -> (snapshot::Snapshot, Value) {
    let only = a
        .all("only")
        .iter()
        .flat_map(|x| x.split(','))
        .map(String::from)
        .collect::<Vec<_>>();
    let mut snap = snapshot::read_cached(&*c.board, &c.cfg, &c.raw, &only, prefetch_comments(c));
    let local = dot(&c.cfg.root).join("status.json");
    if let Some(s) = data::json(&local)
        .ok()
        .flatten()
        .filter(|_| snapshot::status_live(&local, c.cfg.ttl))
    {
        snap.overlay_admission(&s);
    }
    (
        snap,
        reports::decide(&reports::cached(&c.cfg.root), reports::now()),
    )
}

/// One project's text report and the hosts it saw; `t` gets each section's wall time (#2291 `--timing`).
pub(crate) fn status_text(
    c: &Ctx,
    a: &Args,
    read: Option<(snapshot::Snapshot, Value)>,
    t: &mut Vec<(&'static str, f64)>,
) -> Res<(String, Vec<(String, usize)>)> {
    let (b, cfg) = (c.board.as_ref(), &c.cfg);
    let mut clock = std::time::Instant::now();
    let mut lap = |t: &mut Vec<(&'static str, f64)>, name| {
        t.push((name, clock.elapsed().as_secs_f64()));
        clock = std::time::Instant::now();
    };
    let local = dot(&cfg.root).join("status.json");
    let mut out = vec![];
    // R6: a board rate-limit pause the local service reports comes first, also when this read fails
    if read.is_none() {
        if let Some(w) = data::json(&local)
            .ok()
            .flatten()
            .filter(|_| snapshot::status_live(&local, cfg.ttl))
            .as_ref()
            .and_then(requests::warning)
        {
            out.push(w);
        }
    }
    let (snap, decide) = match read {
        Some(r) => r,
        None => {
            let snap = status_snapshot(c, a).0;
            lap(t, "board read");
            let decide = reports::decide(&reports::cached(&cfg.root), reports::now());
            lap(t, "decide (cached)");
            (snap, decide)
        }
    };
    // R6 A3: an unreadable deploy wait or board quota is an anomaly row, never an `unknown` line
    let mut extra = vec![];
    // § 2 Deploy: closed tasks still waiting for their first deploy, a live board read
    let deploys = deploy::status(b, deploy::config(&cfg.root).as_ref()).unwrap_or_else(|e| {
        extra.push(format!(
            "- deploy wait unreadable: {} · Next: rerun `taskq status`; check the board token",
            compact(&e, 160)
        ));
        vec![]
    });
    lap(t, "deploy wait");
    let quota = requests::quota(&b.quota());
    lap(t, "quota");
    if cfg.board.core_budget && quota.starts_with("Board quota: unknown") {
        extra.push("- board quota unread: no core budget headers on this read · Next: rerun `taskq status`; a lasting gap is a board client defect".into());
    }
    out.push(snap.text_extra(cfg, &c.raw, Some(&decide), extra)?);
    lap(t, "render");
    out.extend(deploys);
    if !quota.starts_with("Board quota: unknown") {
        out.push(quota);
    }
    // § 6: the live service's open setup gaps follow the report
    out.extend(
        data::json(&local)
            .ok()
            .flatten()
            .filter(|_| snapshot::status_live(&local, cfg.ttl))
            .map(|s| check::status_lines(&s["setup"]))
            .unwrap_or_default(),
    );
    lap(t, "setup gaps");
    Ok((out.join("\n"), snap.served()))
}

/// R10: `taskq status` covers every project of `<install>/projects` plus the current one; `--project P..` narrows to
/// folder names or `owner/repo`. One project keeps the single-project command; several print one block each.
pub(crate) fn json_here_only(json: bool, wanted: &[String], here: bool) -> bool {
    json && wanted.is_empty() && here
}

pub(crate) fn status_all(a: &Args) -> Res<()> {
    let here = find_root().ok();
    let mut roots = host::projects(&platform::default_install()).unwrap_or_default();
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if let Some(h) = &here {
        roots.retain(|r| canon(r) != canon(h));
        roots.insert(0, h.clone());
    }
    let wanted: Vec<String> = a
        .all("project")
        .iter()
        .flat_map(|x| x.split([',', ' ']))
        .filter(|x| !x.is_empty())
        .map(String::from)
        .collect();
    // A JSON consumer inside a project (tray, PilotQ) reads that project's schema 1 snapshot, not a slow all-project read.
    if json_here_only(a.flags.contains_key("json"), &wanted, here.is_some()) {
        roots.truncate(1);
    }
    if !wanted.is_empty() {
        roots.retain(|r| {
            wanted
                .iter()
                .any(|w| *w == project_name(r) || load(r).is_ok_and(|(cfg, _)| cfg.repo == *w))
        });
        if roots.is_empty() {
            return Err(format!(
                "--project {}: no listed project matches",
                wanted.join(" ")
            ));
        }
    }
    let at = |root: &PathBuf| -> Res<Ctx> {
        let (cfg, raw) = load(root)?;
        let board = cfg.board.open(
            &cfg.web_host,
            &cfg.repo,
            raw["label_schema"].as_str(),
            &platform::tool(&cfg.install),
        )?;
        Ok(Ctx { cfg, raw, board })
    };
    // R10 A1: hosts serving projects outside this view close the report as anomaly rows
    let unseen = || {
        let rows = snapshot::unseen(&SERVED.take());
        if !rows.is_empty() {
            println!("\nAnomalies:\n{}", rows.join("\n"));
        }
    };
    if roots.len() <= 1 {
        let c = match roots.first() {
            Some(r) => at(r)?,
            None => ctx()?,
        };
        command(&c, "status", a)?;
        if !a.flags.contains_key("json") {
            unseen();
        }
        return Ok(());
    }
    if a.flags.contains_key("json") {
        let mut served = vec![];
        let projects: Vec<Value> = roots
            .iter()
            .map(|r| match at(r) {
                Ok(c) => {
                    let (snap, decide) = status_snapshot(&c, a);
                    served.push(snap.served());
                    let mut v = snap.json();
                    if let Ok(t) = snap.text_with(&c.cfg, &c.raw, Some(&decide)) {
                        v["report"] = json!(t);
                    }
                    v["project"] = json!(project_name(r));
                    v["decide"] = decide;
                    v
                }
                Err(e) => json!({"project": project_name(r), "complete": false, "reason": e}),
            })
            .collect();
        return json_output(
            &serde_json::to_string_pretty(
                &json!({"projects": projects, "unseen": snapshot::unseen(&served)}),
            )
            .map_err(|e| e.to_string())?,
            a.one("json-file"),
        );
    }
    // #2291: projects read in parallel; each block prints in list order
    let timing = a.flags.contains_key("timing");
    let started = std::time::Instant::now();
    let prefer = crate::board::local::preferred();
    let blocks: Vec<(String, Vec<(&'static str, f64)>)> = std::thread::scope(|s| {
        let handles: Vec<_> = roots.iter().map(|r| s.spawn(move || {
            crate::board::local::prefer(prefer);
            PROJECT_ROOT.with_borrow_mut(|p| *p = Some(r.clone()));
            let clock = std::time::Instant::now();
            let mut t = vec![];
            let c = at(r);
            t.push(("open", clock.elapsed().as_secs_f64()));
            // a project that cannot be read is an anomaly line of its own block, never a silent gap
            let text = match c.and_then(|c| status_text(&c, a, None, &mut t)) {
                Ok((text, served)) => { SERVED.with_borrow_mut(|v| v.push(served)); (text, SERVED.take()) }
                Err(e) => (format!("{} · source unavailable: {e} · Next: `taskq status --project {}` after fixing its config or board token", project_name(r), project_name(r)), vec![]),
            };
            (text, t)
        })).collect();
        handles
            .into_iter()
            .map(|h| {
                let ((text, served), t) = h
                    .join()
                    .unwrap_or_else(|_| (("status thread panicked".into(), vec![]), vec![]));
                SERVED.with_borrow_mut(|v| v.extend(served));
                (text, t)
            })
            .collect()
    });
    for (i, (text, _)) in blocks.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!("{text}");
    }
    if timing {
        eprintln!(
            "| Project | {} | total |",
            blocks
                .iter()
                .flat_map(|b| b.1.iter().map(|x| x.0))
                .fold(Vec::<&str>::new(), |mut v, n| {
                    if !v.contains(&n) {
                        v.push(n);
                    }
                    v
                })
                .join(" | ")
        );
        for (r, (_, t)) in roots.iter().zip(&blocks) {
            eprintln!(
                "| {} | {} | {:.2} |",
                project_name(r),
                t.iter()
                    .map(|(_, s)| format!("{s:.2}"))
                    .collect::<Vec<_>>()
                    .join(" | "),
                t.iter().map(|x| x.1).sum::<f64>()
            );
        }
        eprintln!("wall {:.2} s", started.elapsed().as_secs_f64());
    }
    PROJECT_ROOT.with_borrow_mut(|p| *p = None);
    unseen();
    Ok(())
}

/// `status`: the rate-limit warning, then closed history, the JSON snapshot or the text report.
/// JSON stdout stays one document: a budget warning goes to stderr there.
pub(crate) fn status_prints_json(a: &Args) -> bool {
    a.flags.contains_key("json") || a.flags.contains_key("closed")
}

pub(crate) fn status_cmd(c: &Ctx, a: &Args) -> Res<()> {
    let (b, cfg) = (c.board.as_ref(), &c.cfg);
    // R6: a board rate-limit pause the local service reports comes first, also when this read fails
    let local = dot(&cfg.root).join("status.json");
    if let Some(w) = data::json(&local)
        .ok()
        .flatten()
        .filter(|_| snapshot::status_live(&local, cfg.ttl))
        .as_ref()
        .and_then(requests::warning)
    {
        if status_prints_json(a) {
            eprintln!("{w}");
        } else {
            println!("{w}");
        }
    }
    if a.flags.contains_key("closed") {
        let page = a
            .one("page")
            .map_or(Ok(1), |p| p.parse::<u32>().map_err(|_| "bad --page"))?;
        let since = a
            .one("since")
            .map(|t| t.parse::<i64>().map_err(|_| "bad --since: board seconds"))
            .transpose()?;
        let value = if let Some(n) = a.one("task") {
            let n = n
                .parse::<u64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or("bad --task")?;
            if a.flags.contains_key("transcript") {
                let before = a
                    .one("before")
                    .map(|v| v.parse::<u64>().map_err(|_| "bad --before"))
                    .transpose()?;
                json!({"transcript":closed_transcript(b,&cfg.root,n,&cfg.me,before,|host,attempt|conversations::closed_remote(&cfg.install,&cfg.root,host,n,attempt,before)).map_or_else(|reason|json!({"reason":reason}),|v|v)})
            } else {
                reports::selected(&cfg.root, b, n, !cfg.board.closed_reason, None)?
            }
        } else {
            closed_history(b, page, since, !cfg.board.closed_reason)?
        };
        println!("{value}");
        return Ok(());
    }
    let (snap, decide) = status_snapshot(c, a);
    if a.flags.contains_key("json") {
        let mut v = snap.json();
        if let Ok(t) = snap.text_with(cfg, &c.raw, Some(&decide)) {
            v["report"] = json!(t);
        }
        v["decide"] = decide;
        json_output(
            &serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?,
            a.one("json-file"),
        )?;
    } else {
        let (text, served) = status_text(c, a, Some((snap, decide)), &mut vec![])?;
        SERVED.with_borrow_mut(|v| v.push(served));
        println!("{text}");
    }
    Ok(())
}
