//! Texts a command writes: task bodies, cards, results and attachments (§ 1, § 2, § 5, § 7).

use crate::{board, core, reports, runtime, service, snapshot};
use crate::Res;
use crate::board::Board;
use crate::core::{Entry, State, Task, entry, fold};
use std::path::Path;

pub(crate) fn numbered(opts: &[String], recommend: Option<usize>) -> String {
    opts.iter().enumerate().map(|(i, o)| format!("{}. {o}{}", i + 1, if recommend == Some(i + 1) { " ★" } else { "" })).collect::<Vec<_>>().join("\n")
}

pub(crate) fn join(parts: &[&str]) -> String {
    parts.iter().filter(|p| !p.trim().is_empty()).copied().collect::<Vec<_>>().join("\n\n")
}

/// R7: Unicode characters of a line; link targets and `code` spans (exact commands, SHAs, paths) are exempt.
pub(crate) fn width(l: &str) -> usize {
    let mut s = String::new();
    let mut rest = l;
    while let Some(a) = rest.find('`') {
        s.push_str(&rest[..a]);
        match rest[a + 1..].find('`') {
            Some(b) => rest = &rest[a + b + 2..],
            None => { rest = &rest[a..]; break; }
        }
    }
    s.push_str(rest);
    while let Some(a) = s.find("](") {
        let b = s[a..].find(')').map_or(s.len(), |b| a + b + 1);
        s.replace_range(a + 1..b, "");
    }
    s.split_inclusive(char::is_whitespace).filter(|w| !w.starts_with("http://") && !w.starts_with("https://")).map(|w| w.chars().count()).sum()
}

/// Rejects, with a one-line hint, a field outside `min..=max` lines or with a line over `chars` (R7: never cut).
pub(crate) fn limit(what: &str, lines: &[String], min: usize, max: usize, chars: usize) -> Res<()> {
    match lines.len() {
        n if n > 0 && max == 0 => Err(format!("{what}: not allowed here (only with --option)")),
        n if n < min || n > max => Err(format!("{what}: {n} lines, allowed {min}-{max}; shorten or split")),
        _ => match lines.iter().map(|l| width(l)).enumerate().find(|(_, w)| *w > chars) {
            Some((i, w)) => Err(format!("{what} line {}: {w} characters, limit {chars}; shorten it", i + 1)),
            None => Ok(()),
        },
    }
}

/// All violations of `checks` in one error, `; `-joined, so one retry fixes them (#1346).
pub(crate) fn all(checks: impl IntoIterator<Item = Res<()>>) -> Res<()> {
    let errs: Vec<String> = checks.into_iter().filter_map(Result::err).collect();
    if errs.is_empty() { Ok(()) } else { Err(errs.join("; ")) }
}

pub(crate) fn one_line(what: &str, items: &[String]) -> Res<()> {
    if items.iter().any(|s| s.contains(['\r', '\n']) || s.trim().is_empty()) { Err(format!("{what}: one nonempty line each")) } else { Ok(()) }
}

/// § 1: `add` numbers Acceptance itself; a typed `A<n>.` label would double (#1696).
pub(crate) fn labelled_acceptance(items: &[String]) -> Res<()> {
    let bad = |s: &str| s.trim_start().strip_prefix('A').and_then(|r| r.split_once('.')).is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()));
    match items.iter().position(|s| bad(s)) {
        Some(i) => Err(format!("--acceptance {}: drop the A<n>. label; add numbers the lines", i + 1)),
        None => Ok(()),
    }
}

/// The `later` arguments (§ 7 rule 3): a worker park names one blocker issue or, when only time is left, a UTC time;
/// only the owner or PM parks without either. `staged` is the current UTC time of a `--stage` park.
pub(crate) fn park<'a>(n: u64, blocker: Option<&'a str>, until: Option<&'a str>, staged: Option<&'a str>, task: Option<String>, attempt: Option<String>) -> Res<Vec<&'a str>> {
    let worker = attempt.is_some() && task.is_some_and(|t| t.parse() == Ok(n));
    match (blocker, until) {
        (Some(_), Some(_)) => return Err("--blocker and --until exclude each other: a park waits for one of them".into()),
        (None, Some(u)) if !core::utc(u) => return Err(format!("bad --until {u}: one UTC time like 2026-11-05T09:00:00Z")),
        (None, Some(u)) => return Ok(vec!["until", u]),
        _ => {}
    }
    // § 2 Stages: a worker stage park alone wakes at once, so the next stage starts without a human step (#2289)
    match blocker {
        None if worker && let Some(now) = staged => Ok(vec!["until", now]),
        None if worker => Err("a worker park needs --blocker #M (file the blocker as a held task first) or, when only time is left, --until <UTC> (§ 7)".into()),
        None => Ok(vec![]),
        Some(_) if !worker => Err("--blocker is for a worker park; the owner or PM adds `Depends on #M` or parks with later".into()),
        Some(r) if !core::blocker_ref(r) || r == format!("#{n}") => Err(format!("bad --blocker {r}: one #M or <repo>#M, not this task")),
        Some(r) => Ok(vec![r]),
    }
}

/// § 7 rule 3 (#2288): the local `#M` of a worker park when it is an open held task, which the park starts.
pub(crate) fn held_blocker(b: &dyn Board, r: &str) -> Option<u64> {
    let m = r.strip_prefix('#')?.parse().ok()?;
    b.issue(m).is_ok_and(|i| i.open && service::is_held(&i)).then_some(m)
}

/// § 2 Stages: `text` with the `Stages:` line of `later --stage`; each stage is an Acceptance item of `body`.
pub(crate) fn stage_text(text: &str, staged: &[String], body: &str) -> Res<String> {
    let ids = core::acceptance(body);
    if let Some(bad) = staged.iter().find(|s| !ids.contains(s)) {
        return Err(format!("bad --stage {bad}: one Acceptance item of this task ({})", if ids.is_empty() { "none".into() } else { ids.join(" ") }));
    }
    let line = core::stages_line(&format!("Stages: {}", staged.join(" "))).unwrap_or_default().join(" ");
    Ok(if text.trim().is_empty() { format!("Stages: {line}") } else { format!("{}\nStages: {line}", text.trim_end()) })
}

pub(crate) fn task_text(cmd: &str, text: &str) -> Res<()> {
    if matches!(cmd, "answer" | "steer") {
        if text.trim().is_empty() { return Err(format!("{cmd}: empty text; type a reply, then send again")); }
    } else if matches!(cmd, "requeue" | "later" | "failed") {
        if text.is_empty() && cmd != "failed" { return Ok(()); }
        let lines: Vec<String> = text.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect();
        all([limit(cmd, &lines, 1, 2, 240), fields(text, &[("Reason", 1, 1, 240), ("Next", 0, 1, 240)]).map(drop)])?;
    }
    Ok(())
}

/// § 5: every human reply reaches the board through this Unicode-safe wrapper, without a line-count cap.
pub(crate) fn wrap_reply(text: &str) -> String {
    text.split('\n').map(|line| {
        let mut rest = line;
        let mut lines = Vec::new();
        while let Some((end, _)) = rest.char_indices().nth(snapshot::ANSWER_CHARS) {
            let split = rest[..end].char_indices().rev().find(|(i, c)| *i > 0 && c.is_whitespace());
            let (at, skip) = split.map_or((end, 0), |(i, c)| (i, c.len_utf8()));
            lines.push(&rest[..at]);
            rest = &rest[at + skip..];
        }
        lines.push(rest);
        lines.join("\n")
    }).collect::<Vec<_>>().join("\n")
}

/// Labelled `--text` lines (`Label: ..`; an unlabelled line continues the field above) in `spec` order:
/// (label, min lines, max lines, characters).
pub(crate) fn fields(text: &str, spec: &[(&str, usize, usize, usize)]) -> Res<Vec<Vec<String>>> {
    let names = spec.iter().map(|s| format!("`{}:`", s.0)).collect::<Vec<_>>().join(", ");
    let mut out = vec![vec![]; spec.len()];
    let mut cur: Option<usize> = None;
    for l in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        match spec.iter().position(|s| l.strip_prefix(s.0).is_some_and(|r| r.starts_with(": "))) {
            Some(i) if cur.is_none_or(|c| i > c || (i == c && spec[i].0 == "Checks")) => cur = Some(i),
            Some(_) => return Err(format!("--text fields go once each in the order {names}")),
            None if cur.is_none() => return Err(format!("--text starts with one of {names}")),
            None => {}
        }
        out[cur.unwrap()].push(l.to_string());
    }
    all(out.iter().zip(spec).map(|(f, s)| limit(s.0, f, s.1, s.2, s.3)))?;
    Ok(out)
}

/// `--link`: `[label](url)` as given, else the URL labelled `#N` by its trailing number, else `link`.
pub(crate) fn evidence(l: &str) -> String {
    if l.starts_with('[') && l.contains("](") || l.starts_with("![") {
        return l.to_string();
    }
    let tail = l.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
    format!("[{}]({l})", if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) { format!("#{tail}") } else { "link".into() })
}

/// § 2 ask card: a Problem or option line holds at most one `[label](url)` with a label of at most 40 characters, no bare URL.
pub(crate) fn card_links(what: &str, lines: &[String]) -> Res<()> {
    all(lines.iter().enumerate().map(|(i, l)| {
        let n = l.matches("](").count();
        let long = l.split("](").take(n).any(|a| a.rsplit('[').next().is_none_or(|t| t.chars().count() > 40));
        let bare = l.split_whitespace().any(|w| w.starts_with("http://") || w.starts_with("https://"));
        match () {
            _ if n > 1 => Err(format!("{what} line {}: {n} links, at most one [label](url)", i + 1)),
            _ if long => Err(format!("{what} line {}: link label at most 40 characters", i + 1)),
            _ if bare => Err(format!("{what} line {}: bare URL; write [label](url)", i + 1)),
            _ => Ok(()),
        }
    }))
}

/// § 2 ask card: an option is `<label> - + <gain>; - <cost>`; the label names the meaning in at most 32 characters, unique.
pub(crate) fn option_lines(opts: &[String]) -> Res<()> {
    let label = |o: &str| o.trim().strip_prefix("- ").map_or_else(|| o.split_once(" - ").map_or(o.trim(), |p| p.0.trim()).to_string(), |_| String::new());
    all(opts.iter().enumerate().map(|(i, o)| {
        let (k, l) = (i + 1, label(o));
        let detail = o.trim().get(l.len()..).unwrap_or_default().trim_start().strip_prefix("- ").unwrap_or_default();
        let pair = detail.strip_prefix("+ ").and_then(|d| d.split_once("; - ")).is_some_and(|(g, c)| !g.trim().is_empty() && !c.trim().is_empty());
        let twin = opts[..i].iter().position(|p| label(p).to_lowercase() == l.to_lowercase());
        if l.is_empty() {
            Err(format!("option line {k}: missing label; write `<label> - + <gain>; - <cost>`"))
        } else if width(&l) > 32 {
            Err(format!("option line {k}: label {} characters, limit 32", width(&l)))
        } else if let Some(j) = twin {
            Err(format!("option line {k}: label repeats option line {}", j + 1))
        } else if !pair {
            Err(format!("option line {k}: detail must be `+ <gain>; - <cost>` after `{l} - `"))
        } else {
            Ok(())
        }
    }))
}

/// § 2 visual evidence: `--image` files, at most four images or videos (#1844) of at most 10 MB each.
pub(crate) fn image_files(paths: &[String]) -> Res<()> {
    if paths.len() > 4 { return Err("Evidence: at most four --image".into()); }
    all(paths.iter().map(|p| {
        let head = std::fs::File::open(p).and_then(|f| { let mut b = vec![]; std::io::Read::read_to_end(&mut std::io::Read::take(f, 16), &mut b).map(|_| b) }).map_err(|e| format!("--image {p}: {e}"))?;
        let video = board::video_kind(&head);
        if board::image_kind(&head).is_none() && video.is_none() { return Err(format!("--image {p}: not a PNG, JPEG, GIF, WebP, MP4 or WebM file")); }
        // boards pick the player by the extension
        if let Some(k) = video.filter(|k| !p.to_ascii_lowercase().ends_with(&format!(".{k}"))) {
            return Err(format!("--image {p}: a {} video's file name ends in .{k}", k.to_uppercase()));
        }
        let size = std::fs::metadata(p).map_err(|e| format!("--image {p}: {e}"))?.len();
        if size > 10 << 20 { return Err(format!("--image {p}: {size} bytes, at most 10 MB")); }
        Ok(())
    }))
}

/// § 2 visual evidence: uploads each `--image` to the board and embeds it as `![name](absolute URL)` only after the board
/// serves the same bytes back (the load check); a failed check publishes nothing. A video the board does not play
/// inline (GitHub) is the link `[▶ name](URL)` that opens it playable (#1844).
pub(crate) fn upload_images(b: &dyn board::Board, n: u64, paths: &[String]) -> Res<Vec<String>> {
    image_files(paths)?;
    paths.iter().map(|p| {
        let url = b.upload_file(n, Path::new(p))?;
        let sent = std::fs::read(p).map_err(|e| format!("--image {p}: {e}"))?;
        let back = b.file(&url).map_err(|e| format!("image load check failed for {url}: {e}"))?;
        if back != sent { return Err(format!("image load check failed: {url} serves {} bytes, {} uploaded", back.len(), sent.len())); }
        let video = board::video_kind(&sent).is_some();
        let name: String = Path::new(p).file_stem().map(|s| s.to_string_lossy().replace([']', '[', ',', ')'], " ")).unwrap_or_default().chars().take(30).collect();
        let name = if !name.trim().is_empty() { name } else if video { "video".into() } else { "image".into() };
        Ok(if video && !b.video_inline() { format!("[▶ {name}]({url})") } else { format!("![{name}]({url})") })
    }).collect()
}

/// § 2 evidence files (#2019): the kinds `result --file` and `ask --file` take, by name ending and leading bytes.
pub(crate) const FILE_KINDS: [(&str, &[&[u8]]); 6] = [(".zip", &[b"PK\x03\x04", b"PK\x05\x06"]), (".tar.gz", &[b"\x1f\x8b"]), (".tgz", &[b"\x1f\x8b"]),
    (".pdf", &[b"%PDF-"]), (".html", &[b"<"]), (".htm", &[b"<"])];

/// § 2 evidence files (#2019): each `--file` of a result or ask is a zip, tar.gz, PDF or HTML file of at most 25 MB,
/// checked before any write; returns the Evidence label of each, `⬇ <name>` of at most 32 characters. `zip_html`
/// (a board that shows HTML as source) labels an HTML file `⬇ <name>.html.zip`, the zip it uploads instead.
pub(crate) fn attachment_files(paths: &[String], zip_html: bool) -> Res<Vec<String>> {
    paths.iter().map(|p| {
        let lower = p.to_ascii_lowercase();
        let (ext, heads) = FILE_KINDS.iter().find(|(e, _)| lower.ends_with(e)).ok_or(format!("--file {p}: not a .zip, .tar.gz, .tgz, .pdf, .html or .htm file"))?;
        let size = std::fs::metadata(p).map_err(|e| format!("--file {p}: {e}"))?.len();
        if size > 25 << 20 { return Err(format!("--file {p}: {size} bytes, at most 25 MB")); }
        let head = std::fs::File::open(p).and_then(|f| { let mut b = vec![]; std::io::Read::read_to_end(&mut std::io::Read::take(f, 1024), &mut b).map(|_| b) }).map_err(|e| format!("--file {p}: {e}"))?;
        // HTML may start with a byte order mark and blank lines before its first tag
        let html = *ext == ".html" || *ext == ".htm";
        let lead = if html { head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&head).trim_ascii_start() } else { &head[..] };
        if !heads.iter().any(|h| lead.starts_with(h)) { return Err(format!("--file {p}: content is not a {} file", &ext[1..])); }
        let name = Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let zip = if html && zip_html { ".zip" } else { "" };
        let stem: String = name[..name.len() - ext.len()].replace(['[', ']', ',', '(', ')'], " ").split_whitespace().collect::<Vec<_>>().join(" ").chars().take(30 - ext.len() - zip.len()).collect();
        Ok(format!("⬇ {}{}{zip}", if stem.trim().is_empty() { "file" } else { stem.trim() }, &name[name.len() - ext.len()..]))
    }).collect()
}

/// § 2 evidence files (#2019): uploads each `--file` to the board's file store and links it as `[⬇ name](absolute URL)`
/// in the Evidence line only after the board serves the same bytes back (the load check); a failed check publishes
/// nothing. On a board that shows HTML as source, an HTML file goes up inside `<name>.zip`, so one click downloads it.
pub(crate) fn upload_attachments(b: &dyn board::Board, n: u64, paths: &[String]) -> Res<Vec<String>> {
    let zip_html = !b.html_downloads();
    paths.iter().zip(attachment_files(paths, zip_html)?).map(|(p, label)| {
        let mut sent = std::fs::read(p).map_err(|e| format!("--file {p}: {e}"))?;
        let html = [".html", ".htm"].iter().any(|e| p.to_ascii_lowercase().ends_with(e));
        let url = if zip_html && html {
            let name = Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            sent = zip_one(&name, &sent)?;
            let dir = std::env::temp_dir().join(format!("taskq-file-{}", runtime::uuid()));
            let zip = dir.join(format!("{name}.zip"));
            let up = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&zip, &sent)).map_err(|e| format!("--file {p}: {e}"))
                .and_then(|_| b.upload_attachment(n, &zip).map_err(String::from));
            let _ = std::fs::remove_dir_all(&dir);
            up?
        } else { b.upload_attachment(n, Path::new(p))? };
        let back = b.file(&url).map_err(|e| format!("file load check failed for {url}: {e}"))?;
        if back != sent { return Err(format!("file load check failed: {url} serves {} bytes, {} uploaded", back.len(), sent.len())); }
        Ok(format!("[{label}]({url})"))
    }).collect()
}

/// A zip archive holding the one file `name` with `data`, deflated, for a board that would show it as text (#2019).
pub(crate) fn zip_one(name: &str, data: &[u8]) -> Res<Vec<u8>> {
    use std::io::Write;
    let mut z = flate2::write::DeflateEncoder::new(vec![], flate2::Compression::default());
    let packed = z.write_all(data).and_then(|_| z.finish()).map_err(|e| format!("zip {name}: {e}"))?;
    let mut crc = flate2::Crc::new();
    crc.update(data);
    let sizes = [packed.len() as u32, data.len() as u32];
    // version 2.0, UTF-8 name flag, deflate, 1980-01-01 00:00; sizes fit u32 under the 25 MB cap
    let entry = |sig: u32| [&sig.to_le_bytes()[..], &20u16.to_le_bytes(), &0x0800u16.to_le_bytes(), &8u16.to_le_bytes(), &0u16.to_le_bytes(),
        &33u16.to_le_bytes(), &crc.sum().to_le_bytes(), &sizes[0].to_le_bytes(), &sizes[1].to_le_bytes(),
        &(name.len() as u16).to_le_bytes(), &0u16.to_le_bytes()].concat();
    let local = [entry(0x0403_4b50), name.as_bytes().to_vec(), packed].concat();
    // the central entry adds "made by" before the shared fields, then comment, disk, attributes and the local offset
    let shared = entry(0x0201_4b50);
    let central = [&shared[..4], &20u16.to_le_bytes(), &shared[4..], &[0u8; 14], name.as_bytes()].concat();
    let end = [&0x0605_4b50u32.to_le_bytes()[..], &[0u8; 4], &1u16.to_le_bytes(), &1u16.to_le_bytes(),
        &(central.len() as u32).to_le_bytes(), &(local.len() as u32).to_le_bytes(), &0u16.to_le_bytes()].concat();
    Ok([local, central, end].concat())
}

/// The Evidence links `--file` will add, with a placeholder URL: the card checks them before any upload.
pub(crate) fn file_stubs(b: &dyn board::Board, paths: &[String]) -> Res<Vec<String>> {
    Ok(attachment_files(paths, !b.html_downloads())?.into_iter().map(|l| format!("[{l}](https://board.invalid/file)")).collect())
}

/// § 2 evidence files (#2019): an uploaded `--file` link, `[⬇ name](url)`.
pub(crate) fn attachment(l: &str) -> bool {
    l.starts_with("[⬇ ")
}

/// § 2 visual evidence: where the name starts in an uploaded image or video, `![name](url)` or `[▶ name](url)`; none
/// for a link.
pub(crate) fn media(l: &str) -> Option<usize> {
    ["![", "[▶ "].into_iter().find(|p| l.starts_with(p)).map(str::len)
}

/// § 2 visual evidence (#1844): media reaches a task only through `--image`. A hand-written image or video link that is
/// relative, host-less (csgo#415: `/-/project/39/uploads/..`) or a repository path renders as a missing file.
pub(crate) fn media_links(values: &[&str]) -> Res<()> {
    const MEDIA: [&str; 9] = ["png", "jpg", "jpeg", "gif", "webp", "mp4", "webm", "mov", "m4v"];
    let bare = values.iter().flat_map(|v| v.split_whitespace()).map(|w| {
        let w = w.rsplit_once("src=").or_else(|| w.rsplit_once("href=")).map_or(w, |x| x.1);
        w.trim_matches(|c: char| "\"'<>()[],;.".contains(c))
    }).filter(|w| w.starts_with('/') || w.starts_with("http://") || w.starts_with("https://"));
    let marked = values.iter().flat_map(|v| v.split("](").skip(1)).filter_map(|r| r.split([')', ' ']).next());
    for link in marked.chain(bare) {
        let path = link.split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
        if !path.rsplit_once('.').is_some_and(|(_, e)| MEDIA.contains(&e)) { continue; }
        let web = path.strip_prefix("https://").or_else(|| path.strip_prefix("http://"));
        // `/raw/<sha>/` is a commit-pinned taskq evidence file; `/raw/<branch>/` moves with the branch
        let pinned = |r: &str| r.split('/').next().is_some_and(|s| s.len() == 40 && s.bytes().all(|c| c.is_ascii_hexdigit()));
        let repo = web.is_some_and(|w| w.starts_with("raw.githubusercontent.com/") || ["/blob/", "/-/blob/", "/-/raw/", "/-/tree/", "/tree/"].iter().any(|m| w.contains(m))
            || w.split_once("/raw/").is_some_and(|(_, r)| !pinned(r)));
        let why = if web.is_none() { "relative or host-less" } else if repo { "a repository path" } else { continue };
        return Err(format!("media link {link} is {why}; attach the file with --image (it uploads and embeds the absolute URL)"));
    }
    Ok(())
}

/// § 2 ask card: Done, Problem, optional Evidence, 2-4 options with one ★ or none (an owner action), the answer line.
pub(crate) fn ask_card(n: u64, text: &str, opts: &[String], recommend: Option<&str>, links: &[String]) -> Res<String> {
    card(n, text, opts, recommend, links, false)
}

/// § 2 ask card of a service decision (retry limit, blocked publication, closed task, refused start, failed input,
/// #2012): the writer of `ask`, so a format change reaches every card TaskQ posts. `done` and `problem` become one
/// plain line each, cut to the field limit; options keep their key (`later:`, `close:`), which runs only through
/// `taskq answer N.K` (§ 2). It never fails: a text the writer refuses is replaced by the refusal.
pub(crate) fn service_card(n: u64, done: &str, problem: &str, opts: &[String], recommend: usize) -> String {
    let text = |p: &str| format!("Done: {}\nProblem: {}", plain(done, 160), plain(p, 240));
    let k = recommend.to_string();
    card(n, &text(problem), opts, Some(&k), &[], true).unwrap_or_else(|e| {
        let opts = ["retry: retry the step - + runs it again; - may fail the same way".into(), "later: park the task - + nothing runs; - waits until the owner requeues it".into()];
        card(n, &text(&format!("card check failed: {e}")), &opts, Some("1"), &[], true).unwrap_or(e)
    })
}

/// One line of at most `max` characters for a service card field: no link (`](` breaks), each URL as `code`.
pub(crate) fn plain(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ").replace("](", "] (");
    let s = if s.is_empty() { "-".into() } else if s.chars().count() > max { format!("{}…", s.chars().take(max - 1).collect::<String>()) } else { s };
    s.split(' ').map(|w| if w.starts_with("http://") || w.starts_with("https://") { format!("`{w}`") } else { w.into() }).collect::<Vec<_>>().join(" ")
}

/// The § 2 card writer of every card TaskQ posts. A `cli` card (a service decision) takes `taskq answer N.K` only:
/// its `close` and `later` options never run from a bare comment `K`.
pub(crate) fn card(n: u64, text: &str, opts: &[String], recommend: Option<&str>, links: &[String], cli: bool) -> Res<String> {
    let f = fields(text, &[("Done", 1, 1, 160), ("Problem", 1, 2, 240)])?;
    all([card_links("Problem", &f[1]), card_links("option", opts), option_lines(opts)])?;
    let mut lines = f.concat();
    one_line("--option", opts)?;
    let action = opts.is_empty() && recommend.is_none();
    if !action && !(2..=4).contains(&opts.len()) || opts.iter().any(|o| o.contains('★')) {
        return Err(format!("{} options, allowed 2-4 without ★ (none: an owner action); --recommend adds it", opts.len()));
    }
    let k = if action { 0 } else {
        recommend.and_then(|k| k.parse().ok()).filter(|k| (1..=opts.len()).contains(k)).ok_or(format!("--recommend needs one option number 1-{}", opts.len()))?
    };
    one_line("--link", links)?;
    if links.iter().filter(|l| media(l).is_none()).count() > 2 {
        return Err("Evidence: at most two --link and --file together".into());
    }
    let ev: Vec<String> = links.iter().filter(|l| media(l).is_none()).map(|l| evidence(l)).collect();
    if ev.iter().any(|e| e.split_once("](").is_some_and(|(l, _)| l.chars().count() > 41)) {
        return Err("Evidence: link labels at most 40 characters".into());
    }
    // § 2: a link must open from the board and every app; a relative path renders as a broken repository link (#1782)
    if let Some(e) = ev.iter().find(|e| !e.split_once("](").is_some_and(|(_, u)| u.starts_with("https://") || u.starts_with("http://"))) {
        return Err(format!("Evidence: {e} needs an absolute http(s) URL"));
    }
    // visual choice (answer card § Question types): one image per option names image K `K. <name>`
    let images: Vec<(&String, usize)> = links.iter().filter_map(|l| Some((l, media(l)?))).collect();
    let each = images.len() >= 2 && images.len() == opts.len();
    let ev = [ev, images.iter().enumerate().map(|(i, (l, k))| if each { format!("{}{}. {}", &l[..*k], i + 1, &l[*k..]) } else { l.to_string() }).collect()].concat();
    if !ev.is_empty() {
        lines.push(format!("Evidence: {}", ev.join(", ")));
    }
    if action {
        lines.push(format!("To answer: `taskq answer {n} --text <done>`."));
        return Ok(lines.join("\n"));
    }
    let opts: Vec<String> = numbered(opts, Some(k)).lines().map(String::from).collect();
    limit("option", &opts, 2, 4, 160)?;
    lines.extend(opts);
    lines.push(if cli {
        format!("To answer: `taskq answer {n}.K`; a bare comment `K` is a plain answer, never `close` or `later`.")
    } else {
        format!("To answer: `taskq answer {n}.K` or comment `K` or the option text.")
    });
    Ok(lines.join("\n"))
}

/// § 7 result: Changed, one `Checks:` line per check, Open; a decision appends the § 2 card (Done/Problem in `--text`).
pub(crate) fn result_text(n: u64, text: &str, checks: &[String], opts: &[String], recommend: Option<&str>, links: &[String]) -> Res<String> {
    let d = !opts.is_empty() as usize;
    if d == 0 && (recommend.is_some() || links.iter().any(|l| media(l).is_none() && !attachment(l))) {
        return Err("--recommend and --link need --option".into());
    }
    let f = fields(text, &[("Changed", 1, 2, 240), ("Open", 1, 2, 240), ("Done", d, d, 160), ("Problem", d, 2 * d, 240)])?;
    let checks: Vec<String> = checks.iter().flat_map(|c| c.lines()).map(str::trim).filter(|c| !c.is_empty()).map(|c| format!("Checks: {}", c.trim_start_matches("Checks: "))).collect();
    limit("Checks", &checks, 1, 8, 240)?;
    let verdict = [f[0].clone(), checks, f[1].clone()].concat();
    limit("result", &verdict, 3, 12, 240)?;
    if links.iter().filter(|l| media(l).is_none()).count() > 2 {
        return Err("Evidence: at most two --link and --file together".into());
    }
    let card = if d == 1 { ask_card(n, &[f[2].clone(), f[3].clone()].concat().join("\n"), opts, recommend, links)? } else if links.is_empty() { String::new() } else { format!("Evidence: {}", links.join(", ")) };
    Ok(join(&[&verdict.join("\n"), &card]))
}

/// § 7 close verdict: Changed, Checks naming every Acceptance item `A<k> ok|gap|unknown` and CI, Open.
/// § 7: deferred publication accepts merge-result CI; synchronous routes name the result SHA.
pub(crate) fn close_check(text: &str, issue: &board::Issue, t: &Task, deferred: bool) -> Res<()> {
    if t.state != State::Review || text.is_empty() {
        return Ok(());
    }
    close_verdict(text, &issue.body, reports::experiment(issue))?;
    if deferred && text.lines().any(|l| l.starts_with("Checks:") && l.contains("CI") && (l.contains("merge-result") || l.contains("merge_group"))) {
        return Ok(());
    }
    if t.result.as_deref().is_some_and(|sha| text.lines().any(|l| l.starts_with("Checks:") && l.contains(sha))) { Ok(()) } else { Err("Checks: name the exact result SHA with the CI outcome".into()) }
}

/// #1979: an experiment's close carries one `Verdict: confirmed|refuted|inconclusive <evidence>` line after Changed.
pub(crate) fn close_verdict(text: &str, body: &str, experiment: bool) -> Res<()> {
    let lines: Vec<String> = text.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect();
    limit("verdict", &lines, 3, 12, 240)?;
    let f = fields(text, &[("Changed", 1, 2, 240), ("Verdict", 0, 1, 240), ("Checks", 1, 8, 240), ("Open", 1, 2, 240)])?;
    limit("verdict", &f.concat(), 3, 12, 240)?;
    if (experiment || !f[1].is_empty()) && reports::outcome_verdict(&f[1].join("\n")).is_none() {
        return Err("Verdict: an experiment closes with `Verdict: confirmed|refuted|inconclusive <evidence>` after Changed".into());
    }
    let checks = f[2].join(" ");
    for k in body.lines().filter_map(|l| l.trim().strip_prefix('A')?.split_once('.')?.0.parse::<u32>().ok()) {
        if !["ok", "gap", "unknown"].iter().any(|v| checks.contains(&format!("A{k} {v}"))) {
            return Err(format!("Checks: name A{k} ok|gap|unknown"));
        }
    }
    if checks.contains("CI") { Ok(()) } else { Err("Checks: name the route's CI outcome".into()) }
}

/// Selftest of the rendered forms: a result, a close verdict, and an ask card answered by the hand-typed form (§ 2).
pub(crate) fn formats_selftest() -> Res<String> {
    let checks = ["cargo test --locked -- pass".to_string()];
    result_text(1, "Changed: x\nOpen: none", &checks, &[], None, &[])?;
    close_verdict("Changed: x\nChecks: A1 ok; CI green on the SHA\nOpen: none", "A1. y", false)?;
    let card = ask_card(1, "Done: not started\nProblem: pick one", &["Left - + fast; - ugly".into(), "Right - + clean; - slow".into()], Some("2"), &[])?;
    let log: Vec<Entry> = [entry("spawn", &["worker", &format!("{}:s", runtime::OWNER), "mac"], None, ""), entry("ask", &[], Some(1), &card), "taskq answer 2".into()]
        .into_iter().enumerate().map(|(i, body)| Entry { id: i as u64 + 1, body, edited: false }).collect();
    match (fold(1, &log, false).state, core::delivery(1, &log, 3)) {
        (State::Doing, d) if d == "Right - + clean; - slow" => Ok("formats: ask card, result, close verdict PASS".into()),
        (s, d) => Err(format!("formats: FAIL hand-typed answer gave {} and {d:?}", s.name())),
    }
}

pub(crate) fn cell(s: &str) -> String {
    s.replace('\\', "\\\\").replace('|', "\\|").replace('[', "\\[").replace(']', "\\]").replace(['\r', '\n'], " ")
}

/// R6: compact display only; the issue link retains the complete stored text.
pub(crate) fn compact(s: &str, max: usize) -> String {
    let s = s.replace(['\r', '\n'], " ");
    if s.chars().count() > max { s.chars().take(max - 1).chain(['…']).collect() } else { s }
}
