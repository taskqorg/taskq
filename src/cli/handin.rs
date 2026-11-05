//! Hand-in commands: ask, result and later (§ 2, § 7).

use crate::{Res, board, reports, respond};
use crate::cli::Args;
use crate::cli::task_cmd::start_tasks;
use crate::cli::text::{ask_card, attachment, file_stubs, held_blocker, image_files, media, park, result_text, stage_text, upload_attachments, upload_images};
use crate::config::Ctx;
use crate::dispatch::{Op, act};

/// `ask`: validates the card and its files before any upload, then posts it.
pub(crate) fn ask_cmd(c: &Ctx, a: &Args, op: &Op) -> Res<()> {
    let (b, root) = (c.board.as_ref(), c.cfg.root.as_path());
    let json = a.flags.contains_key("json");
    let n = a.num(1)?;
    let (links, images, files) = (a.all("link"), a.all("image"), a.all("file"));
    if links.iter().any(|l| l.starts_with('!') || media(l).is_some() || attachment(l)) { return Err("--link takes links; images and videos go through --image, files through --file".into()); }
    // validate the card and the files before any upload
    ask_card(n, a.need("text")?, &a.all("option"), a.one("recommend"), &[links.clone(), file_stubs(b, &files)?].concat())?;
    image_files(&images)?;
    let body = ask_card(n, a.need("text")?, &a.all("option"), a.one("recommend"), &[links, upload_attachments(b, n, &files)?, upload_images(b, n, &images)?].concat())?;
    respond(a, &[act(b, root, n, "ask", &[], &body, op)], json)
}

/// `result`: validates the text and its files before any upload, then posts the result for the SHA.
pub(crate) fn result_cmd(c: &Ctx, a: &Args, op: &Op) -> Res<()> {
    let (b, root) = (c.board.as_ref(), c.cfg.root.as_path());
    let text = a.one("text").unwrap_or("");
    let json = a.flags.contains_key("json");
    let n = a.num(1)?;
    let sha = a.need("sha")?;
    let (links, images, files) = (a.all("link"), a.all("image"), a.all("file"));
    if links.iter().any(|l| l.starts_with('!') || media(l).is_some() || attachment(l)) { return Err("--link takes links; images and videos go through --image, files through --file".into()); }
    result_text(n, text, &a.all("checks"), &a.all("option"), a.one("recommend"), &[links.clone(), file_stubs(b, &files)?].concat())?;
    image_files(&images)?;
    let report = reports::prepare(b, root, n, sha)?;
    let mut body = result_text(n, text, &a.all("checks"), &a.all("option"), a.one("recommend"), &[links, upload_attachments(b, n, &files)?, upload_images(b, n, &images)?].concat())?;
    if report {body+=&format!("\nReport: docs/research/{n}.md");if body.lines().filter(|l|!l.trim().is_empty()).count()>12 {return Err("report result exceeds 12 lines; shorten the verdict".into());}}
    respond(a, &[act(b, root, n, "result", &[sha], &body, op)], json)
}

/// `later`: parks the task, recording any finished stages, and starts a held local blocker.
pub(crate) fn later_cmd(c: &Ctx, a: &Args, op: &Op) -> Res<()> {
    let (b, root) = (c.board.as_ref(), c.cfg.root.as_path());
    let text = a.one("text").unwrap_or("");
    let json = a.flags.contains_key("json");
    let n = a.num(1)?;
    let staged = a.all("stage");
    let now = board::stamp(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64));
    let args = park(n, a.one("blocker"), a.one("until"), (!staged.is_empty()).then_some(now.as_str()), std::env::var("TASKQ_TASK").ok(), std::env::var("TASKQ_ATTEMPT").ok())?;
    let text = if staged.is_empty() { text.to_string() } else { stage_text(text, &staged, &b.issue(n)?.body)? };
    let mut out = vec![act(b, root, n, "later", &args, &text, op)];
    // § 7 rule 3 (#2288): the worker's held local blocker starts with its park, so neither waits for the PM
    if let Some(m) = a.one("blocker").filter(|_| out[0].ok()).and_then(|r| held_blocker(b, r)) {
        out.extend(start_tasks(b, root, &[m.to_string()], &Op::default())?);
    }
    respond(a, &out, json)
}
