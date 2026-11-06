//! §7 evidence outcomes: owner binding, read-only proof, and no source adapter calls.
use crate::{
    Res,
    board::{self, Board, Issue},
    core::{self, Entry, Task},
    dispatch::{Op, Receipt},
};
use serde::{Deserialize, Serialize};
use std::path::Path;

const PREFIX: &str = "taskq: evidence-authorized\n";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Authorization {
    task: u64,
    board: String,
    candidate: String,
    baseline: String,
    result: u64,
    revision: u64,
    through: u64,
    scope: String,
    url: String,
    sha256: String,
    op: String,
    #[serde(default)]
    source_record: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<Source>,
}
/// Explicit owner-selected canonical archive, separate from retained evidence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub url: String,
    pub sha256: String,
}
fn hash(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn scope(issue: &Issue) -> Res<String> {
    let types: Vec<_> = issue
        .labels
        .iter()
        .filter(|s| matches!(s.as_str(), "code" | "research" | "docs" | "asset"))
        .collect();
    if types.len() != 1 || types[0] == "code" {
        return Err("evidence completion requires one non-code task type".into());
    }
    Ok(hash(
        format!("{}\n{}\n{}", issue.title, issue.body, types[0]).as_bytes(),
    ))
}
fn hex(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn git_bytes(root: &Path, args: &[&str]) -> Res<Vec<u8>> {
    let mut c = crate::platform::command("git");
    c.arg("-C")
        .arg(root)
        .args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args);
    let output = coreq::process::output("evidence.git", &mut c, std::time::Duration::from_secs(10))
        .map_err(|_| "evidence workspace observation unknown".to_string())?;
    if !output.status.success() {
        return Err("evidence source observation failed or unknown".into());
    }
    Ok(output.stdout)
}
fn git(root: &Path, args: &[&str]) -> Res<String> {
    String::from_utf8(git_bytes(root, args)?)
        .map_err(|_| "evidence workspace identity unreadable".into())
}
fn workspace(root: &Path, n: u64, candidate: &str, baseline: &str) -> Res<()> {
    if !hex(candidate, 40) || !hex(baseline, 40) {
        return Err("evidence candidate/baseline requires full lowercase SHA".into());
    }
    let name = format!("taskq-{n}");
    let dir = if root.file_name().is_some_and(|s| s == name.as_str())
        && root
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|s| s == ".worktrees")
    {
        root.to_path_buf()
    } else {
        root.join(".worktrees").join(name)
    };
    let branch = format!("refs/heads/taskq-{n}");
    if git(&dir, &["symbolic-ref", "HEAD"])?.trim() != branch
        || git(&dir, &["rev-parse", "HEAD"])?.trim() != candidate
    {
        return Err("evidence task worktree does not name the candidate".into());
    }
    // A missing/truncated creation record is unknown, never an inferred baseline.
    let reflog = git(&dir, &["reflog", "show", "--format=%H %gs", &branch])?;
    let created = reflog
        .lines()
        .last()
        .ok_or("evidence attempt baseline unknown")?;
    if !created.starts_with(&format!("{baseline} branch: Created from ")) {
        return Err("evidence attempt baseline changed or unknown".into());
    }
    if !git(&dir, &["status", "--porcelain", "--untracked-files=all"])?
        .trim()
        .is_empty()
        || !git(
            &dir,
            &[
                "diff",
                "--no-ext-diff",
                "--name-only",
                baseline,
                candidate,
                "--",
            ],
        )?
        .trim()
        .is_empty()
    {
        return Err("evidence task has a source delta".into());
    }
    Ok(())
}
/// Reads one small identity member, never extracts an archive or executes its contents.
/// The ZIP directory, local header, sizes and CRC must agree; ZIP64/encryption/streaming refuse.
fn zip_record(bytes: &[u8], name: &str) -> Res<String> {
    let data = zip_member(bytes, name)?;
    if data.len() > 65536 {
        return Err("archive source record oversized".into());
    }
    String::from_utf8(data).map_err(|_| "archive source record unreadable".into())
}
fn zip_member(bytes: &[u8], name: &str) -> Res<Vec<u8>> {
    use std::io::Read;
    let bad = || "archive source record missing, corrupt or unsupported".to_string();
    if name.is_empty()
        || name.contains(char::is_control)
        || name.starts_with('/')
        || name.contains('\\')
        || name
            .trim_end_matches('/')
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
    {
        return Err(bad());
    }
    let u16at = |at: usize| -> Res<usize> {
        Ok(u16::from_le_bytes(
            bytes
                .get(at..at + 2)
                .ok_or_else(bad)?
                .try_into()
                .map_err(|_| bad())?,
        ) as usize)
    };
    let u32at = |at: usize| -> Res<usize> {
        Ok(u32::from_le_bytes(
            bytes
                .get(at..at + 4)
                .ok_or_else(bad)?
                .try_into()
                .map_err(|_| bad())?,
        ) as usize)
    };
    let end = zip_end(bytes)?;
    if bytes.get(end..end + 4) != Some(b"PK\x05\x06")
        || u16at(end + 4)? != 0
        || u16at(end + 6)? != 0
    {
        return Err(bad());
    }
    let count = u16at(end + 10)?;
    let mut at = u32at(end + 16)?;
    if count == 65535 || u16at(end + 8)? != count || at.checked_add(u32at(end + 12)?) != Some(end) {
        return Err(bad());
    }
    let mut found = None;
    for _ in 0..count {
        if bytes.get(at..at + 4) != Some(b"PK\x01\x02") {
            return Err(bad());
        }
        let len = u16at(at + 28)?;
        let extra = u16at(at + 30)?;
        let comment = u16at(at + 32)?;
        let entry = bytes.get(at + 46..at + 46 + len).ok_or_else(bad)?;
        if entry == name.as_bytes() {
            if found.is_some() || u16at(at + 8)? & !0x0806 != 0 || u16at(at + 34)? != 0 {
                return Err(bad());
            }
            let method = u16at(at + 10)?;
            let crc = u32at(at + 16)? as u32;
            let compressed = u32at(at + 20)?;
            let size = u32at(at + 24)?;
            if size > 25 * 1024 * 1024 {
                return Err(bad());
            }
            let local = u32at(at + 42)?;
            if local >= at
                || bytes.get(local..local + 4) != Some(b"PK\x03\x04")
                || u16at(local + 6)? != u16at(at + 8)?
                || u16at(local + 8)? != method
                || u32at(local + 14)? != crc as usize
                || u32at(local + 18)? != compressed
                || u32at(local + 22)? != size
                || u16at(local + 26)? != len
                || bytes.get(local + 30..local + 30 + len) != Some(entry)
            {
                return Err(bad());
            }
            let start = local + 30 + len + u16at(local + 28)?;
            let finish = start
                .checked_add(compressed)
                .filter(|f| *f <= at)
                .ok_or_else(bad)?;
            let data = bytes.get(start..finish).ok_or_else(bad)?;
            let decoded = match method {
                0 => data.to_vec(),
                8 => {
                    let mut out = Vec::new();
                    flate2::read::DeflateDecoder::new(data)
                        .take(25 * 1024 * 1024 + 1)
                        .read_to_end(&mut out)
                        .map_err(|_| bad())?;
                    out
                }
                _ => return Err(bad()),
            };
            let mut actual = flate2::Crc::new();
            actual.update(&decoded);
            if decoded.len() != size || actual.sum() != crc {
                return Err(bad());
            }
            found = Some(decoded);
        }
        at = at
            .checked_add(46 + len + extra + comment)
            .filter(|v| *v <= end)
            .ok_or_else(bad)?;
    }
    if at != end {
        return Err(bad());
    }
    found.ok_or_else(bad)
}
// Git archives may carry the immutable commit in their EOCD comment.
fn zip_end(bytes: &[u8]) -> Res<usize> {
    let start = bytes.len().saturating_sub(65557);
    let mut ends = (start..bytes.len().saturating_sub(21)).filter(|&i| {
        bytes.get(i..i + 4) == Some(b"PK\x05\x06")
            && i + 22 + u16::from_le_bytes([bytes[i + 20], bytes[i + 21]]) as usize == bytes.len()
    });
    let end = ends.next().ok_or("source ZIP directory missing")?;
    if ends.next().is_some() {
        return Err("source ZIP directory ambiguous".into());
    }
    Ok(end)
}
fn archive_files(bytes: &[u8], prefixed: bool) -> Res<std::collections::BTreeMap<String, Vec<u8>>> {
    let bad = || "canonical source ZIP corrupt, ambiguous or unsupported".to_string();
    let end = zip_end(bytes)?;
    let count = u16::from_le_bytes(bytes[end + 10..end + 12].try_into().unwrap()) as usize;
    let mut at = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
    let mut files = std::collections::BTreeMap::new();
    let mut prefix = None;
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0usize;
    for _ in 0..count {
        let header = bytes
            .get(at..at.checked_add(46).ok_or_else(bad)?)
            .ok_or_else(bad)?;
        if &header[..4] != b"PK\x01\x02" {
            return Err(bad());
        }
        let field = |i| u16::from_le_bytes([header[i], header[i + 1]]) as usize;
        let len = field(28);
        let name = std::str::from_utf8(bytes.get(at + 46..at + 46 + len).ok_or_else(bad)?)
            .map_err(|_| bad())?;
        if !seen.insert(name.to_string()) {
            return Err(bad());
        }
        let data = zip_member(bytes, name)?;
        total = total
            .checked_add(data.len())
            .filter(|v| *v <= 128 * 1024 * 1024)
            .ok_or_else(bad)?;
        let path = if prefixed {
            let (top, rest) = name.split_once('/').ok_or_else(bad)?;
            if prefix.as_deref().is_some_and(|v| v != top) {
                return Err(bad());
            }
            prefix = Some(top.to_string());
            rest
        } else {
            name
        };
        if !path.is_empty() && !path.ends_with('/') && files.insert(path.into(), data).is_some() {
            return Err(bad());
        }
        at = at
            .checked_add(46 + len + field(30) + field(32))
            .filter(|v| *v <= end)
            .ok_or_else(bad)?;
    }
    if at != end || (prefixed && count == 0) {
        return Err(bad());
    }
    Ok(files)
}
fn archival_source(b: &dyn Board, root: &Path, a: &Authorization, source: &Source) -> Res<()> {
    if a.source_record.is_some()
        || !hex(&a.candidate, 40)
        || !hex(&a.baseline, 40)
        || !source.url.starts_with("https://")
        || source.url.contains(char::is_whitespace)
        || source.url == a.url
        || !hex(&source.sha256, 64)
    {
        return Err("separate archival source identity invalid".into());
    }
    let data = b
        .file(&source.url)
        .map_err(|_| "canonical source archive read unknown")?;
    if data.is_empty() || data.len() > 25 * 1024 * 1024 || hash(&data) != source.sha256 {
        return Err("canonical source archive missing or changed".into());
    }
    // No extraction or interpretation of downloaded files; Git reads only its owned objects.
    let expected = git_bytes(root, &["archive", "--format=zip", &a.candidate])?;
    let expected = archive_files(&expected, false)?;
    if archive_files(&data, false)? != expected && archive_files(&data, true)? != expected {
        return Err("canonical source archive differs from candidate".into());
    }
    if root
        .join(format!(".worktrees/taskq-{}", a.task))
        .try_exists()
        .map_err(|_| "evidence workspace existence unknown")?
        || root
            .file_name()
            .is_some_and(|s| s == format!("taskq-{}", a.task).as_str())
    {
        workspace(root, a.task, &a.baseline, &a.baseline)?;
    }
    Ok(())
}
fn result(n: u64, log: &[Entry], t: &Task) -> Res<u64> {
    let p = core::project(n, log, false);
    p.changes
        .iter()
        .rev()
        .find(|(id, _, after)| {
            after.result == t.result
                && log
                    .iter()
                    .any(|e| e.id == *id && core::kind(n, &e.body) == "result")
        })
        .map(|(id, _, _)| *id)
        .ok_or("evidence result identity unknown".into())
}
fn names(text: &str, url: &str) -> bool {
    text.split(|c: char| c.is_whitespace() || matches!(c, '"' | '(' | ')' | '<' | '>'))
        .any(|s| s == url)
}
fn validate(
    b: &dyn Board,
    root: &Path,
    issue: &Issue,
    t: &Task,
    log: &[Entry],
    a: &Authorization,
) -> Res<()> {
    let n = issue.number;
    if a.task != n
        || a.board != b.url(n)
        || Some(a.candidate.as_str()) != t.result.as_deref()
        || a.result != result(n, log, t)?
        || a.scope != scope(issue)?
    {
        return Err("evidence authorization no longer matches task/result identity".into());
    }
    let rev = core::revision(n, log);
    let projection = core::project(n, log, false);
    if log.iter().any(|e| {
        e.id > a.through && projection.accepts(e.id) && core::kind(n, &e.body) != "publish"
    }) {
        return Err("evidence authorization outcome changed".into());
    }
    let published = log
        .iter()
        .find(|e| e.id == rev && !e.edited && core::kind(n, &e.body) == "publish");
    if rev != a.revision
        && !published.is_some_and(|p| {
            core::revision(
                n,
                &log.iter()
                    .filter(|e| e.id < p.id)
                    .cloned()
                    .collect::<Vec<_>>(),
            ) == a.revision
        })
    {
        return Err("evidence authorization revision is stale".into());
    }
    if !a.url.starts_with("https://") || a.url.contains(char::is_whitespace) || !hex(&a.sha256, 64)
    {
        return Err("evidence attachment identity invalid".into());
    }
    if !log
        .iter()
        .any(|e| e.id == a.result && names(&e.body, &a.url))
        && !t.publication.as_ref().is_some_and(|v| names(v, &a.url))
    {
        return Err("evidence attachment is absent from the accepted result/verdict".into());
    }
    let bytes = b
        .file(&a.url)
        .map_err(|_| "evidence attachment read unknown".to_string())?;
    if bytes.is_empty() || bytes.len() > 25 * 1024 * 1024 || hash(&bytes) != a.sha256 {
        return Err("evidence attachment missing or changed".into());
    }
    if let Some(source) = &a.source {
        archival_source(b, root, a, source)?;
    } else if let Some(name) = &a.source_record {
        if !hex(&a.candidate, 40) || a.candidate != a.baseline {
            return Err("archive source baseline differs from candidate".into());
        }
        let record = zip_record(&bytes, name)?;
        let source: Vec<_> = record
            .lines()
            .filter(|l| l.split_whitespace().next() == Some("source"))
            .collect();
        if source.len() != 1
            || source[0].split_whitespace().collect::<Vec<_>>()
                != ["source", a.candidate.as_str(), "dirty=", "0"]
        {
            return Err("archive clean-source identity missing or changed".into());
        }
        // Archived proof is explicit, never an implicit fallback from an unknown workspace read.
        if root
            .join(format!(".worktrees/taskq-{n}"))
            .try_exists()
            .map_err(|_| "evidence workspace existence unknown")?
            || root
                .file_name()
                .is_some_and(|s| s == format!("taskq-{n}").as_str())
        {
            workspace(root, n, &a.candidate, &a.baseline)?;
        }
    } else {
        workspace(root, n, &a.candidate, &a.baseline)?;
    }
    let fresh = b.issue(n)?;
    let fresh_log = board::entries(b, n)?;
    if scope(&fresh)? != a.scope
        || fresh_log
            .iter()
            .map(|e| (e.id, &e.body, e.edited))
            .ne(log.iter().map(|e| (e.id, &e.body, e.edited)))
    {
        return Err("evidence task changed during verification".into());
    }
    Ok(())
}
/// An audit selects this path even when invalid: never fall through to source publication.
pub fn completion(
    b: &dyn Board,
    root: &Path,
    issue: &Issue,
    t: &Task,
    log: &[Entry],
) -> Option<Res<String>> {
    let audit = log.iter().rev().find(|e| e.body.starts_with(PREFIX))?;
    Some((|| {
        if audit.edited {
            return Err("evidence authorization was edited".into());
        }
        let a: Authorization = serde_json::from_str(audit.body.strip_prefix(PREFIX).unwrap())
            .map_err(|_| "evidence authorization unreadable".to_string())?;
        if log.iter().any(|e| {
            e.id != audit.id
                && e.body.starts_with(PREFIX)
                && serde_json::from_str::<Authorization>(&e.body[PREFIX.len()..])
                    .is_ok_and(|v| v.op == a.op)
                && (e.edited || e.body != audit.body)
        }) {
            return Err("evidence authorization operation is ambiguous".into());
        }
        validate(b, root, issue, t, log, &a)?;
        let source = a
            .source
            .as_ref()
            .map(|source| {
                format!(
                    "; archival source {} sha256 {}; baseline {}",
                    source.url, source.sha256, a.baseline
                )
            })
            .unwrap_or_default();
        Ok(format!(
            "Completion: evidence-only v1: authorization {}; candidate {}; evidence {} sha256 {}{}; source not published",
            audit.id, a.candidate, a.url, a.sha256, source
        ))
    })())
}
/// Owner-only audit. It neither approves a gate nor changes the fold.
#[allow(clippy::too_many_arguments)]
pub fn authorize(
    b: &dyn Board,
    root: &Path,
    n: u64,
    candidate: &str,
    baseline: &str,
    url: &str,
    sha256: &str,
    op: &Op,
    owner: bool,
    readers: bool,
    source_record: Option<&str>,
    source: Option<Source>,
) -> Receipt {
    let run = || -> Res<Receipt> {
        if !owner || !readers {
            return Err("owner authorization and --readers-current yes required".into());
        }
        let id = op
            .id
            .as_deref()
            .filter(|s| !s.is_empty() && !s.contains(char::is_whitespace))
            .ok_or("evidence requires --op-id")?;
        let revision = op.revision.ok_or("evidence requires --revision")?;
        let issue = b.issue(n)?;
        let log = board::entries(b, n)?;
        let t = core::fold(n, &log, false);
        if let Some(prior) = log.iter().find(|e| {
            e.body.starts_with(PREFIX)
                && serde_json::from_str::<Authorization>(&e.body[PREFIX.len()..])
                    .is_ok_and(|v| v.op == id)
        }) {
            let a: Authorization = serde_json::from_str(&prior.body[PREFIX.len()..])
                .map_err(|_| "evidence authorization unreadable".to_string())?;
            if prior.edited
                || a.task != n
                || a.board != b.url(n)
                || a.candidate != candidate
                || a.baseline != baseline
                || a.url != url
                || a.sha256 != sha256
                || a.revision != revision
                || a.source_record.as_deref() != source_record
                || a.source != source
            {
                return Err("evidence operation identity changed".into());
            }
            return Ok(Receipt::accepted(n, op, t.state, a.revision));
        }
        if !issue.open || !issue.trusted || t.state != core::State::Review {
            return Err("evidence requires an open trusted reviewed result".into());
        }
        let projection = core::project(n, &log, false);
        let a = Authorization {
            task: n,
            board: b.url(n),
            candidate: candidate.into(),
            baseline: baseline.into(),
            result: result(n, &log, &t)?,
            revision,
            through: log
                .iter()
                .filter(|e| projection.accepts(e.id))
                .map(|e| e.id)
                .max()
                .unwrap_or(0),
            scope: scope(&issue)?,
            url: url.into(),
            sha256: sha256.into(),
            op: id.into(),
            source_record: source_record.map(String::from),
            source: source.clone(),
        };
        let body = format!(
            "{PREFIX}{}",
            serde_json::to_string(&a).map_err(|e| e.to_string())?
        );
        if core::revision(n, &log) != revision {
            return Err("evidence authorization revision is stale".into());
        }
        validate(b, root, &issue, &t, &log, &a)?;
        let receipt = b.comment(n, &body)?;
        if !board::entries(b, n)?
            .iter()
            .any(|e| e.id == receipt && !e.edited && e.body == body)
        {
            return Err("evidence authorization readback unknown; retry same operation".into());
        }
        Ok(Receipt::accepted(n, op, t.state, revision).wake(root))
    };
    run().unwrap_or_else(|e| Receipt::rejected(n, op, e))
}

/// Accepted completion identity for projections; never qualifies source publication.
pub fn receipt(n: u64, log: &[Entry]) -> Option<(u64, String)> {
    let p = core::project(n, log, false);
    log.iter()
        .rev()
        .filter(|e| p.accepts(e.id) && core::kind(n, &e.body) == "close")
        .find_map(|e| {
            e.body
                .lines()
                .find(|l| l.starts_with("Completion: evidence-only v1:"))
                .map(|l| (e.id, l.to_string()))
        })
}

/// Stable completion operation survives a lost POST response without a second receipt.
pub fn operation(proof: &str, revision: u64) -> Op {
    Op {
        id: Some(format!("evidence-close-{}", hash(proof.as_bytes()))),
        revision: Some(revision),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        board::Fake,
        core::{State, entry},
        dispatch,
    };
    struct Fixture {
        b: Fake,
        root: std::path::PathBuf,
        n: u64,
        sha: String,
        cfg: crate::service::Config,
    }
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("taskq-evidence-{}", crate::runtime::uuid()));
            std::fs::create_dir_all(&root).unwrap();
            let run = |args: &[&str]| git(&root, args).unwrap();
            run(&["init", "-q"]);
            run(&[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "invented baseline",
            ]);
            let sha = run(&["rev-parse", "HEAD"]).trim().to_string();
            let dir = root.join(".worktrees/taskq-1");
            run(&[
                "worktree",
                "add",
                "-q",
                "-b",
                "taskq-1",
                dir.to_str().unwrap(),
                &sha,
            ]);
            std::fs::write(root.join("taskq.json"), r#"{"repo":"invented/evidence"}"#).unwrap();
            let (mut cfg, _) = crate::config::load(&root).unwrap();
            cfg.install = root.join("install");
            let b = Fake::default();
            let n = b
                .create(
                    "Evidence fixture",
                    "## Goal\nqualify an immutable candidate",
                    &["taskq".into(), "research".into()],
                )
                .unwrap();
            let a = b
                .comment(
                    n,
                    &entry("spawn", &["worker", "codex:invented", "fixture"], None, ""),
                )
                .unwrap();
            b.comment(n,&entry("result",&[&sha],Some(a),"Changed: collected evidence https://board.example/evidence.zip\nOpen: native coverage remains limited")).unwrap();
            b.comment(
                n,
                &entry(
                    "spawn",
                    &["reviewer", "codex:invented-review", "fixture"],
                    None,
                    "",
                ),
            )
            .unwrap();
            b.state.borrow_mut().images.push((
                "https://board.example/evidence.zip".into(),
                b"invented evidence".to_vec(),
            ));
            Self {
                b,
                root,
                n,
                sha,
                cfg,
            }
        }
        fn log(&self) -> Vec<Entry> {
            board::entries(&self.b, self.n).unwrap()
        }
        fn task(&self) -> Task {
            core::fold(self.n, &self.log(), false)
        }
        fn authorize(&self) -> Receipt {
            authorize(
                &self.b,
                &self.root,
                self.n,
                &self.sha,
                &self.sha,
                "https://board.example/evidence.zip",
                &hash(b"invented evidence"),
                &Op {
                    id: Some("invented-authorization".into()),
                    revision: Some(core::revision(self.n, &self.log())),
                },
                true,
                true,
                None,
                None,
            )
        }
        fn save(&self) {
            self.b
                .comment(
                    self.n,
                    &entry(
                        "publish",
                        &[],
                        None,
                        "Changed: collected evidence\nOpen: native limitations retained",
                    ),
                )
                .unwrap();
        }
        fn resume(&self) -> Receipt {
            dispatch::resume_publication(
                &self.b,
                &self.cfg,
                &self.b.issue(self.n).unwrap(),
                &self.task(),
                &self.log(),
            )
        }
        fn initial(&self) -> Receipt {
            dispatch::close_one(
                &self.b,
                &self.cfg,
                self.n,
                "Changed: evidence checked\nOpen: native limitations retained",
                &Op::default(),
            )
        }
        fn closes(&self) -> usize {
            self.log()
                .iter()
                .filter(|e| !e.edited && core::kind(self.n, &e.body) == "close")
                .count()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    // §7/R5: production dispatch, real bounded Git observations, substituted board bytes.
    // Risk: off-main evidence enters source publication, or restart repeats the close/input.
    #[test]
    fn evidence_completion_initial_and_restart_never_publish_source() {
        for saved in [false, true] {
            let f = Fixture::new();
            f.b.state.borrow_mut().publish = "superseded".into();
            if saved {
                f.save();
            }
            assert!(f.authorize().ok());
            let receipt = if saved { f.resume() } else { f.initial() };
            assert!(receipt.ok(), "{}", receipt.text());
            assert_eq!(f.task().state, State::Closed);
            assert_eq!(f.closes(), 1);
            assert_eq!(f.b.state.borrow().publications, 0);
            let log = f.log();
            let summary = crate::summary::text(
                &f.b,
                f.n,
                &f.b.issue(f.n).unwrap().body,
                &log,
                &f.task(),
                None,
            );
            assert!(summary.contains("evidence-only"));
            let close = log
                .iter()
                .find(|e| core::kind(f.n, &e.body) == "close")
                .unwrap();
            assert!(
                close.body.contains("Completion: evidence-only v1:")
                    && close.body.contains(&f.sha)
                    && close.body.contains(&hash(b"invented evidence"))
            );
            assert!(!close.body.contains("already on main"));
            assert_eq!(
                crate::deploy::delivered(f.n, &log),
                None,
                "evidence completion cannot announce a source deployment"
            );
            let source = crate::readiness::source(f.n, &f.b.url(f.n), &log);
            assert_eq!(source["accepted_result_state"], "proven");
            assert_eq!(source["state"], "unknown");
            assert!(!f.resume().ok());
            assert_eq!(f.closes(), 1);
        }
        let f = Fixture::new();
        f.save();
        assert!(f.authorize().ok());
        f.b.state.borrow_mut().lose_comment_response = true;
        assert_eq!(f.resume().outcome, "unknown");
        // Rebuild exclusively from board entries, as after restart; no launch interface is involved.
        assert_eq!(f.task().state, State::Closed);
        assert!(!f.resume().ok());
        assert_eq!(f.closes(), 1);
        assert_eq!(f.b.state.borrow().publications, 0);
        let mut restarted = crate::service::Service::new(
            &f.b,
            &f.cfg,
            "fixture",
            Box::new(|_| panic!("evidence restart must not launch or replay input")),
        );
        restarted.pass().unwrap();
        assert_eq!(f.closes(), 1);
        assert_eq!(f.b.state.borrow().publications, 0);
        crate::service::project(&f.b, &f.b.issue(f.n).unwrap(), &f.task()).unwrap();
        assert!(!f.b.issue(f.n).unwrap().open);
    }

    // A2/A3: fail closed at both adapters; stale authorization is never a missing-PR exemption.
    #[test]
    fn evidence_completion_refuses_changed_unknown_and_unapproved_identity() {
        for saved in [false, true] {
            for fault in [
                "bytes",
                "missing",
                "scope",
                "type",
                "code",
                "candidate",
                "baseline",
                "dirty",
                "edited",
                "malformed",
                "revision",
                "steer",
                "gate",
                "stale-gate",
            ] {
                let f = Fixture::new();
                if saved {
                    f.save();
                }
                assert!(f.authorize().ok());
                match fault {
                    "bytes" => f.b.state.borrow_mut().bad_image = true,
                    "missing" => f.b.state.borrow_mut().images.clear(),
                    "scope" => f.b.edit(f.n, "changed goal").unwrap(),
                    "type" => f.b.labels(f.n, &[], &["research".into()]).unwrap(),
                    "code" => {
                        f.b.labels(f.n, &["code".into()], &["research".into()])
                            .unwrap()
                    }
                    "candidate" | "baseline" | "malformed" => {
                        let mut st = f.b.state.borrow_mut();
                        let c = &mut st.comments.last_mut().unwrap().1;
                        let mut a: Authorization =
                            serde_json::from_str(c.body.strip_prefix(PREFIX).unwrap()).unwrap();
                        if fault == "candidate" {
                            a.candidate = "a".repeat(40);
                        } else {
                            a.baseline = "b".repeat(40);
                        }
                        c.body = if fault == "malformed" {
                            format!("{PREFIX}{{}}")
                        } else {
                            format!("{PREFIX}{}", serde_json::to_string(&a).unwrap())
                        };
                    }
                    "dirty" => std::fs::write(
                        f.root.join(".worktrees/taskq-1/untracked"),
                        "invented delta",
                    )
                    .unwrap(),
                    "edited" => f.b.state.borrow_mut().comments.last_mut().unwrap().1.edited = true,
                    "revision" => {
                        if saved {
                            f.b.comment(f.n, "taskq: publication-blocked\nProblem: new decision")
                                .unwrap();
                        } else {
                            f.b.comment(
                                f.n,
                                &entry("ask", &[], f.task().attempt, "Problem: new decision"),
                            )
                            .unwrap();
                        }
                        assert_eq!(f.task().state, State::Ask);
                        f.b.comment(f.n, "taskq: answer\nContinue").unwrap();
                        assert_eq!(f.task().state, State::Review);
                    }
                    "steer" => {
                        f.b.comment(f.n, "taskq: steer\nRecheck changed evidence")
                            .unwrap();
                    }
                    "gate" | "stale-gate" => {}
                    _ => unreachable!(),
                }
                let r = if fault == "gate" || fault == "stale-gate" {
                    let mut cfg = f.cfg.clone();
                    cfg.delivery = crate::config::Delivery::ReviewPr;
                    if saved {
                        let mut t = f.task();
                        t.gate = Some(if fault == "gate" {
                            core::Gate::Required
                        } else {
                            core::Gate::Stale
                        });
                        dispatch::resume_publication(
                            &f.b,
                            &cfg,
                            &f.b.issue(f.n).unwrap(),
                            &t,
                            &f.log(),
                        )
                    } else {
                        dispatch::close_one(
                            &f.b,
                            &cfg,
                            f.n,
                            "Changed: evidence checked\nOpen: none",
                            &Op::default(),
                        )
                    }
                } else if saved {
                    f.resume()
                } else {
                    f.initial()
                };
                assert!(!r.ok(), "{saved} {fault}: {}", r.text());
                assert_eq!(f.closes(), 0, "{fault}");
                assert_eq!(f.b.state.borrow().publications, 0, "{fault}");
            }
        }
        for saved in [false, true] {
            let f = Fixture::new();
            if saved {
                f.save();
            }
            f.b.state.borrow_mut().publish = "superseded".into();
            if saved {
                f.resume();
            } else {
                f.initial();
            }
            assert_ne!(
                f.task().state,
                State::Closed,
                "no authorization must retain source-publication guards"
            );
            assert_eq!(f.closes(), 0);
            assert_eq!(f.b.state.borrow().publications, 1);
        }
        let f = Fixture::new();
        let op = Op {
            id: Some("invented-owner".into()),
            revision: Some(core::revision(f.n, &f.log())),
        };
        for (owner, readers) in [(false, true), (true, false)] {
            assert!(
                !authorize(
                    &f.b,
                    &f.root,
                    f.n,
                    &f.sha,
                    &f.sha,
                    "https://board.example/evidence.zip",
                    &hash(b"invented evidence"),
                    &op,
                    owner,
                    readers,
                    None,
                    None
                )
                .ok()
            );
        }
        assert!(f.authorize().ok());
        assert!(f.authorize().ok());
        assert_eq!(
            f.log()
                .iter()
                .filter(|e| e.body.starts_with(PREFIX))
                .count(),
            1
        );
    }
    // A2: an explicitly named candidate still cannot hide a committed task source delta.
    #[test]
    fn evidence_authorization_refuses_a_committed_source_delta() {
        let f = Fixture::new();
        let dir = f.root.join(".worktrees/taskq-1");
        std::fs::write(dir.join("source.rs"), "// invented task change\n").unwrap();
        git(&dir, &["add", "source.rs"]).unwrap();
        git(
            &dir,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-q",
                "-m",
                "invented source delta",
            ],
        )
        .unwrap();
        let sha = git(&dir, &["rev-parse", "HEAD"]).unwrap();
        let sha = sha.trim();
        f.b.comment(f.n, "taskq: requeue").unwrap();
        let a =
            f.b.comment(f.n, "taskq: spawn worker codex:invented fixture")
                .unwrap();
        f.b.comment(
            f.n,
            &entry(
                "result",
                &[sha],
                Some(a),
                "Changed: invented https://board.example/evidence.zip\nOpen: none",
            ),
        )
        .unwrap();
        let op = Op {
            id: Some("invented-code-delta".into()),
            revision: Some(core::revision(f.n, &f.log())),
        };
        let r = authorize(
            &f.b,
            &f.root,
            f.n,
            sha,
            &f.sha,
            "https://board.example/evidence.zip",
            &hash(b"invented evidence"),
            &op,
            true,
            true,
            None,
            None,
        );
        assert!(!r.ok() && r.reason.contains("source delta"), "{}", r.text());
        assert_eq!(f.b.state.borrow().publications, 0);
        assert_eq!(f.closes(), 0);
    }

    // A4/A5: a frozen clean-clone record supports evidence-only work with no task checkout.
    // Hash/ZIP/source identity mutations refuse; nothing reconstructs a branch or reruns a probe.
    #[test]
    fn evidence_archive_completion_binds_clean_source_without_a_checkout() {
        for fault in [
            "none",
            "missing-record",
            "dirty-record",
            "other-source",
            "corrupt",
            "oversize",
            "truncated",
        ] {
            let f = Fixture::new();
            let dir = f.root.join(".worktrees/taskq-1");
            git(&f.root, &["worktree", "remove", dir.to_str().unwrap()]).unwrap();
            let record = match fault {
                "dirty-record" => format!("source {} dirty= 1\n", f.sha),
                "other-source" => format!("source {} dirty= 0\n", "b".repeat(40)),
                "oversize" => "x".repeat(65537),
                _ => format!("source {} dirty= 0\n", f.sha),
            };
            let name = "invented-evidence/identity-before.txt";
            let mut data = crate::cli::text::zip_one(name, record.as_bytes()).unwrap();
            if fault == "corrupt" {
                data[14] ^= 1;
            }
            if fault == "truncated" {
                data.truncate(data.len() - 1);
            }
            let digest = hash(&data);
            f.b.state.borrow_mut().images[0].1 = data;
            f.save();
            let op = Op {
                id: Some("invented-archive-authority".into()),
                revision: Some(core::revision(f.n, &f.log())),
            };
            let selected = if fault == "missing-record" {
                "invented-evidence/missing.txt"
            } else {
                name
            };
            let r = authorize(
                &f.b,
                &f.root,
                f.n,
                &f.sha,
                &f.sha,
                "https://board.example/evidence.zip",
                &digest,
                &op,
                true,
                true,
                Some(selected),
                None,
            );
            if fault == "none" {
                assert!(r.ok(), "{}", r.text());
                assert!(f.resume().ok());
                assert_eq!(f.closes(), 1);
                let mut restarted = crate::service::Service::new(
                    &f.b,
                    &f.cfg,
                    "fixture",
                    Box::new(|_| panic!("archive completion must not launch a runtime")),
                );
                restarted.pass().unwrap();
                assert_eq!(f.closes(), 1);
            } else {
                assert!(!r.ok(), "{fault}: {}", r.text());
                assert_eq!(f.closes(), 0);
            }
            assert!(!dir.exists());
            assert_eq!(f.b.state.borrow().publications, 0);
        }
    }

    fn retained() -> (Fixture, String, Source) {
        let f = Fixture::new();
        // Candidate source belongs to a separate source outcome. The evidence branch stays
        // at its original creation baseline, with no reset or source commit on that branch.
        std::fs::write(
            f.root.join("candidate.txt"),
            b"invented canonical source\0\xff",
        )
        .unwrap();
        git(&f.root, &["add", "candidate.txt"]).unwrap();
        git(
            &f.root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-qm",
                "invented separate source outcome",
            ],
        )
        .unwrap();
        let candidate = git(&f.root, &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();
        f.b.comment(f.n, "taskq: requeue").unwrap();
        let attempt =
            f.b.comment(f.n, "taskq: spawn worker codex:invented fixture")
                .unwrap();
        f.b.comment(f.n, &entry("result", &[&candidate], Some(attempt),
            "Changed: retained evidence https://board.example/evidence.zip\nOpen: native limitations retained")).unwrap();
        f.b.comment(f.n, "taskq: spawn reviewer codex:invented-review fixture")
            .unwrap();
        let data = git_bytes(
            &f.root,
            &["archive", "--format=zip", "--prefix=canonical/", &candidate],
        )
        .unwrap();
        let source = Source {
            url: "https://board.example/source.zip".into(),
            sha256: hash(&data),
        };
        f.b.state
            .borrow_mut()
            .images
            .push((source.url.clone(), data));
        (f, candidate, source)
    }
    fn retained_authorize(
        f: &Fixture,
        candidate: &str,
        source: Source,
        owner: bool,
        readers: bool,
    ) -> Receipt {
        authorize(
            &f.b,
            &f.root,
            f.n,
            candidate,
            &f.sha,
            "https://board.example/evidence.zip",
            &hash(b"invented evidence"),
            &Op {
                id: Some("invented-retained-authorization".into()),
                revision: Some(core::revision(f.n, &f.log())),
            },
            owner,
            readers,
            None,
            Some(source),
        )
    }

    // §7/R5/R13: retained evidence binds a distinct candidate, creation baseline and source ZIP.
    // Risk: restart or lost responses publish source, replay native input, or replace an artifact.
    #[test]
    fn archival_source_binds_separate_bytes_and_retained_identity() {
        for saved in [false, true] {
            let (f, candidate, source) = retained();
            if saved {
                f.save();
            }
            let before = f.b.state.borrow().images.clone();
            assert_ne!(candidate, f.sha);
            let r = retained_authorize(&f, &candidate, source.clone(), true, true);
            assert!(r.ok(), "{}", r.text());
            assert!(retained_authorize(&f, &candidate, source.clone(), true, true).ok());
            let mut changed = source.clone();
            changed.sha256 = "a".repeat(64);
            assert!(!retained_authorize(&f, &candidate, changed, true, true).ok());
            if saved {
                f.b.state.borrow_mut().lose_comment_response = true;
            }
            let r = if saved { f.resume() } else { f.initial() };
            if saved {
                assert_eq!(r.outcome, "unknown");
            } else {
                assert!(r.ok(), "{}", r.text());
            }
            assert_eq!(f.task().state, State::Closed);
            assert_eq!(f.closes(), 1);
            let receipt = receipt(f.n, &f.log()).unwrap().1;
            assert!(
                receipt.contains(&candidate)
                    && receipt.contains(&source.sha256)
                    && receipt.contains(&f.sha)
            );
            assert_eq!(f.b.state.borrow().images, before);
            assert_eq!(
                git(&f.root.join(".worktrees/taskq-1"), &["rev-parse", "HEAD"])
                    .unwrap()
                    .trim(),
                f.sha
            );
            let mut restarted = crate::service::Service::new(
                &f.b,
                &f.cfg,
                "fixture",
                Box::new(|_| panic!("archival completion must not launch or replay native input")),
            );
            restarted.pass().unwrap();
            assert_eq!(f.closes(), 1);
            assert_eq!(f.b.state.borrow().publications, 0);
            assert!(crate::deploy::delivered(f.n, &f.log()).is_none());
            assert_eq!(
                crate::readiness::source(f.n, &f.b.url(f.n), &f.log())["state"],
                "unknown"
            );
        }
        let (f, candidate, source) = retained();
        let dir = f.root.join(".worktrees/taskq-1");
        git(&f.root, &["worktree", "remove", dir.to_str().unwrap()]).unwrap();
        assert!(retained_authorize(&f, &candidate, source, true, true).ok());
        assert!(f.initial().ok());
        assert!(!dir.exists());
        assert_eq!(f.b.state.borrow().publications, 0);
    }

    // §7: both first close and saved-publication reconciliation freshly verify the same binding.
    // Existing generic guards retain scope/result/revision/owner-gate coverage; these cases
    // add the independent source artifact and distinct-baseline boundary.
    #[test]
    fn archival_source_refuses_changed_proof_dirty_baseline_and_unsupported_readers() {
        for saved in [false, true] {
            for fault in [
                "source-bytes",
                "source-missing",
                "wrong-source",
                "wrong-path",
                "corrupt-source",
                "evidence",
                "baseline",
                "dirty",
                "candidate",
                "scope",
                "stale",
                "edited",
                "record-conflict",
            ] {
                let (f, candidate, source) = retained();
                if saved {
                    f.save();
                }
                assert!(retained_authorize(&f, &candidate, source, true, true).ok());
                match fault {
                    "source-bytes" => f.b.state.borrow_mut().images[1].1.push(0),
                    "source-missing" => {
                        f.b.state.borrow_mut().images.pop();
                    }
                    "wrong-source" | "wrong-path" | "corrupt-source" => {
                        let name = if fault == "wrong-path" {
                            "canonical/other.txt"
                        } else {
                            "canonical/candidate.txt"
                        };
                        let content: &[u8] = if fault == "wrong-path" {
                            b"invented canonical source\0\xff"
                        } else {
                            b"invented changed source"
                        };
                        let mut data = crate::cli::text::zip_one(name, content).unwrap();
                        if fault == "corrupt-source" {
                            data[14] ^= 1;
                        }
                        let mut st = f.b.state.borrow_mut();
                        st.images[1].1 = data.clone();
                        let audit = &mut st.comments.last_mut().unwrap().1;
                        let mut a: Authorization =
                            serde_json::from_str(&audit.body[PREFIX.len()..]).unwrap();
                        a.source.as_mut().unwrap().sha256 = hash(&data);
                        audit.body = format!("{PREFIX}{}", serde_json::to_string(&a).unwrap());
                    }
                    "evidence" => f.b.state.borrow_mut().images[0].1.push(0),
                    "dirty" => std::fs::write(
                        f.root.join(".worktrees/taskq-1/untracked"),
                        b"invented delta",
                    )
                    .unwrap(),
                    "scope" => f.b.edit(f.n, "changed goal").unwrap(),
                    "stale" => {
                        f.b.comment(f.n, "taskq: steer\nChanged outcome").unwrap();
                    }
                    "edited" => f.b.state.borrow_mut().comments.last_mut().unwrap().1.edited = true,
                    "baseline" | "candidate" | "record-conflict" => {
                        let mut st = f.b.state.borrow_mut();
                        let audit = &mut st.comments.last_mut().unwrap().1;
                        let mut a: Authorization =
                            serde_json::from_str(&audit.body[PREFIX.len()..]).unwrap();
                        match fault {
                            "baseline" => a.baseline = candidate.clone(),
                            "candidate" => a.candidate = f.sha.clone(),
                            _ => a.source_record = Some("identity.txt".into()),
                        }
                        audit.body = format!("{PREFIX}{}", serde_json::to_string(&a).unwrap());
                    }
                    _ => unreachable!(),
                }
                let r = if saved { f.resume() } else { f.initial() };
                assert!(!r.ok(), "{saved} {fault}: {}", r.text());
                assert_eq!(f.closes(), 0, "{fault}");
                assert_eq!(f.b.state.borrow().publications, 0, "{fault}");
            }
        }
        let (f, candidate, source) = retained();
        for (owner, readers) in [(false, true), (true, false)] {
            assert!(!retained_authorize(&f, &candidate, source.clone(), owner, readers).ok());
        }
        assert_eq!(
            f.log()
                .iter()
                .filter(|e| e.body.starts_with(PREFIX))
                .count(),
            0
        );
        // Rule approval alone leaves ordinary source-publication gates in force.
        f.b.comment(f.n, "Owner approved the archival provenance rule")
            .unwrap();
        assert!(completion(&f.b, &f.root, &f.b.issue(f.n).unwrap(), &f.task(), &f.log()).is_none());
    }

    // #2473 A2: optional read-only checkpoint of the retained #2463 artifacts, never replacements.
    #[test]
    #[ignore = "requires the original #2463 canonical ZIP and retained evidence archive"]
    fn archival_2463_retained_artifacts_checkpoint() {
        let source = std::fs::read(
            std::env::var("TASKQ_ARCHIVAL_SOURCE").expect("retained canonical ZIP required"),
        )
        .unwrap();
        let evidence = std::fs::read(
            std::env::var("TASKQ_ARCHIVAL_EVIDENCE").expect("retained evidence required"),
        )
        .unwrap();
        assert_eq!(
            hash(&source),
            "c3b6b60d64b9509428bef32d60d28100c8ab10aba458906a621bdf32b2e1d045"
        );
        assert_eq!(
            hash(&evidence),
            "38bbe17dd0d12b64b454942e1c7e970b27bff466e96007494fd6e0806515af1d"
        );
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let candidate = "ee39a45cdaf50f9ebc9d4de3c9a8fc92118b2e34";
        let baseline = "49acb093929ed0bc4db0f4a1722b5465bdb83805";
        assert_ne!(candidate, baseline);
        let expected = archive_files(
            &git_bytes(root, &["archive", "--format=zip", candidate]).unwrap(),
            false,
        )
        .unwrap();
        assert!(
            archive_files(&source, false).unwrap() == expected
                || archive_files(&source, true).unwrap() == expected
        );
        println!(
            "#2463 retained hashes and candidate source match; no authorization, task write, source publication or native launch"
        );
    }

    // A5 read-only development checkpoint: real sanitized receipts/bytes, substituted board only.
    #[test]
    #[ignore = "requires the explicitly supplied #2462 board and attachment snapshots"]
    fn evidence_2462_snapshot_checkpoint() {
        let f = Fixture::new();
        let snapshot =
            std::env::var("TASKQ_EVIDENCE_SNAPSHOT").expect("sanitized board snapshot required");
        let archive = std::env::var("TASKQ_EVIDENCE_ARCHIVE")
            .expect("sanitized attachment snapshot required");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(snapshot).unwrap()).unwrap();
        let data = std::fs::read(archive).unwrap();
        let candidate = "12699ff8eed7d16ac721090654e802648ccc0c63";
        let digest = "b2d942f439bff45ec87e1468e0e1c7aba3052f90af98f529e5cc5f341b7c715d";
        assert_eq!(hash(&data), digest);
        assert_eq!(value["number"], 2462);
        let url = "https://github.com/taskqorg/taskq/raw/1d585528371cda28cdb571e6acd167b3b20a3848/2462/b2d942f439bf-taskq-2462-mac-evidence.zip";
        {
            let mut st = f.b.state.borrow_mut();
            st.issues = vec![Issue {
                number: 2462,
                title: value["title"].as_str().unwrap().into(),
                body: value["body"].as_str().unwrap().into(),
                labels: serde_json::from_value(value["labels"].clone()).unwrap(),
                open: true,
                trusted: true,
                ..Default::default()
            }];
            st.comments = value["comments"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| {
                    (
                        2462,
                        board::Comment {
                            id: c["id"].as_u64().unwrap(),
                            body: c["body"].as_str().unwrap().into(),
                            trusted: c["trusted"] == true,
                            edited: c["edited"] == true,
                            ..Default::default()
                        },
                    )
                })
                .collect();
            st.next_id = st.comments.iter().map(|(_, c)| c.id).max().unwrap();
            st.images = vec![(url.into(), data)];
        }
        let n = 2462;
        let read = || {
            let log = board::entries(&f.b, n).unwrap();
            let t = core::fold(n, &log, false);
            (t, log)
        };
        let (before, _) = read();
        assert_eq!(before.state, State::Ask);
        assert_eq!(before.result.as_deref(), Some(candidate));
        // Simulated owner answer/authorization exist only on this in-memory board, never the live task.
        f.b.comment(
            n,
            "taskq: answer\nPreserve the evidence-only outcome and all limitations",
        )
        .unwrap();
        let (_, log) = read();
        let op = Op {
            id: Some("invented-snapshot-authorization".into()),
            revision: Some(core::revision(n, &log)),
        };
        let r = authorize(
            &f.b,
            &f.root,
            n,
            candidate,
            candidate,
            url,
            digest,
            &op,
            true,
            true,
            Some("taskq-2462-mac-evidence/identity-before.txt"),
            None,
        );
        assert!(r.ok(), "{}", r.text());
        let (t, log) = read();
        let r = dispatch::resume_publication(&f.b, &f.cfg, &f.b.issue(n).unwrap(), &t, &log);
        assert!(r.ok(), "{}", r.text());
        let (t, log) = read();
        assert_eq!(t.state, State::Closed);
        let close = log
            .iter()
            .find(|e| core::kind(n, &e.body) == "close")
            .unwrap();
        assert!(
            core::verdict(n, &log, core::entry_text(n, &close.body))
                .contains("installed-service not qualified")
        );
        assert_eq!(f.b.state.borrow().publications, 0);
        assert_eq!(
            log.iter()
                .filter(|e| core::kind(n, &e.body) == "close")
                .count(),
            1
        );
        assert!(crate::deploy::delivered(n, &log).is_none());
        println!(
            "#2462 snapshot: exact candidate/archive verified; one evidence receipt; zero source publication; no live task or native probe changed"
        );
    }
}
