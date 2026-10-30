//! GitLab issues (taskq.md § 1, § 2, § 7): REST v4 on gitlab.com or a self-managed host, `glab` login.

use super::{Api, Board, image_name, Comment, Issue, Kind, Publish, Result, hex40, into, iso, names, outcome, s, stamp, verify_assignment};
use serde_json::{Value, json};
use std::cell::RefCell;

pub const KIND: Kind = Kind {
    reload_token: false,
    name: "gitlab",
    user_key: "username",
    default_host: "gitlab.com",
    token,
    token_env: &["GITLAB_TOKEN"],
    api,
    board: |api| Box::new(GitLab { api, members: RefCell::new(vec![]) }),
};

/// coreq's lookup (§ 9): env, `<install>/gitlab-token`, then `glab`. Reaching `glab` means no env token and no file.
fn token(tool: &coreq::Tool, host: &str) -> Result<String> {
    tool.token(true, host).map_err(|e| missing(&tool.install.join("gitlab-token"), host, &e))
}

/// Names what to provide when neither the file nor `glab` gave a token (#1894); other errors pass unchanged.
fn missing(file: &std::path::Path, host: &str, e: &str) -> String {
    match e.strip_prefix("board token lookup unavailable: ") {
        Some(why) => format!("no GitLab token: no {} and `glab` cannot run ({why})\nNext: write a GitLab access token for {host} to that file, or install glab and log in to {host}", file.display()),
        None => e.into(),
    }
}

fn api(host: &str, repo: &str, token: &str) -> Api {
    let root = format!("https://{host}/api/v4");
    Api::new(root.clone(), format!("{root}/projects/{}", repo.replace('/', "%2F")), token)
}

pub struct GitLab {
    pub(super) api: Api,
    members: RefCell<Vec<(u64, bool)>>,
}

fn csv(v: &[String]) -> String {
    v.join(",")
}

impl GitLab {
    fn parse_issue(&self, v: &Value) -> Issue {
        Issue {
            number: v["iid"].as_u64().unwrap_or(0),
            title: s(v, "title"),
            body: s(v, "description"),
            labels: names(&v["labels"], "name"),
            assignees: names(&v["assignees"], "username"),
            updated: iso(&s(v, "updated_at")).unwrap_or(0),
            created: iso(&s(v, "created_at")).unwrap_or(0),
            closed: iso(&s(v, "closed_at")),
            open: v["state"] == "opened",
            trusted: self.member(v["author"]["id"].as_u64().unwrap_or(0)),
            // GitLab records no close reason: a closed issue is done (§ 1)
            reason: None,
        }
    }

    /// Reporter+ (access level 20), cached per author id.
    fn member(&self, user: u64) -> bool {
        if let Some((_, ok)) = self.members.borrow().iter().find(|(u, _)| *u == user) {
            return *ok;
        }
        let ok = self.api.call("GET", &format!("/members/all/{user}"), None).ok().and_then(|r| r.json().ok())
            .is_some_and(|v| v["access_level"].as_u64().unwrap_or(0) >= 20);
        self.members.borrow_mut().push((user, ok));
        ok
    }

    /// CI on `sha`: Ok(None) green, Ok(Some) the outcome that stops this close (failed, or still running), Err pending or missing.
    fn ci(&self, sha: &str) -> Result<Option<Publish>> {
        let v = self.api.call("GET", &format!("/repository/commits/{sha}"), None)?.json()?;
        let p = &v["last_pipeline"];
        match p["status"].as_str() {
            Some("success") => Ok(None),
            Some(st @ ("failed" | "canceled")) => Ok(Some(Publish::Failed(format!("CI {st} on {sha}{}", link(p))))),
            Some(st) if RUNNING.contains(&st) => Ok(Some(self.running(&format!("CI on {sha}"), p))),
            Some(st) => Err(format!("CI {st} on {sha}")),
            None => Err(format!("no CI on {sha}")),
        }
    }

    /// § 7 wait on a running pipeline: the service merges once it ends (#1939); past the deadline only the owner can unblock it.
    fn running(&self, what: &str, p: &Value) -> Publish {
        let st = p["status"].as_str().unwrap_or("running");
        // board clock only (§ 4); unknown clock or start never trips the deadline
        let clock = *self.api.clock.borrow();
        let age = iso(&s(p, "created_at")).filter(|_| clock > 0).map(|t| clock - t);
        if age.is_some_and(|a| a > PIPELINE_DEADLINE) {
            return Publish::Blocked(format!("{what} still {st} after {} h{}", PIPELINE_DEADLINE / 3600, link(p)));
        }
        Publish::Waiting(format!("{what} {st}{}: merges when it passes", link(p)))
    }

    fn publication_ancestry(&self, sha: &str, mode: &str) {
        // Only the immediate read in this close qualifies; later main changes are not incidents.
        if !hex40(sha) { return; }
        let on_main = self.api.call("GET", &format!("/repository/merge_base?refs[]=main&refs[]={sha}"), None)
            .and_then(|r| r.json()).and_then(|v| {
                let id = v["id"].as_str().filter(|id| hex40(id)).ok_or("unknown merge base")?;
                Ok(id == sha)
            });
        if on_main == Ok(false) {
            crate::incident::detect("publication.not_on_main", concat!(file!(), ":", line!()), "none", &[mode], &["ancestry:missing"]);
        }
    }

    fn try_publish(&self, n: u64, sha: &str, mode: &str, title: &str, task_line: &str) -> Result<Publish> {
        let branch = format!("taskq-{n}");
        // No commit, or the SHA is already on main: verify ancestry, close.
        let base = self.api.call("GET", &format!("/repository/merge_base?refs[]=main&refs[]={sha}"), None)?.json()?["id"].clone();
        if base == sha {
            return Ok(Publish::Done(format!("{sha} already on main")));
        }
        if let Some(stop) = self.ci(sha)? {
            return Ok(stop);
        }
        // no fast-forward ref API: `direct` merges a fast-forward MR (§ 7)
        let mrs = self.api.call("GET", &format!("/merge_requests?source_branch={branch}&target_branch=main&state=opened"), None)?.json()?;
        let mrs = mrs.as_array().cloned().unwrap_or_default();
        if mrs.is_empty() {
            // § 7 merged, not closed: a merged MR at the result SHA into any target (a release branch too) is the publication; no second merge
            let merged = self.api.call("GET", &format!("/merge_requests?source_branch={branch}&state=merged"), None)?.json()?;
            if let Some(mr) = merged.as_array().into_iter().flatten().find(|m| m["sha"] == sha) {
                return Ok(Publish::Done(format!("merged !{}{}", mr["iid"], into(&s(mr, "target_branch")))));
            }
        }
        let mr = match (mrs.len(), mode) {
            (1, _) => mrs[0].clone(),
            (0, "direct") => self.api.call("POST", "/merge_requests", Some(json!({ "source_branch": branch, "target_branch": "main", "title": title })))?.json()?,
            (k, _) => return Ok(Publish::Failed(format!("{k} open MRs of {branch}, want exactly one"))),
        };
        if mr["sha"] != sha {
            return Ok(Publish::Failed(format!("MR head {} is not the result {sha}", s(&mr, "sha"))));
        }
        if mode == "pr" && base != self.api.call("GET", "/repository/branches/main", None)?.json()?["commit"]["id"] {
            // CI on a head that lacks current main proves nothing about the merge result (#1139)
            return Ok(Publish::Failed(format!("{branch} is not rebased on current main: rebase, rerun the checks and push")));
        }
        let project = self.api.call("GET", "", None)?.json()?;
        if project["autoclose_referenced_issues"] != false {
            // MR or commit text such as 'Do not close #N' would close the task on merge, before its acceptance (csgo#428, #1909)
            return Ok(Publish::Pending("project closes issues named in MR or commit text: set autoclose_referenced_issues to false".into()));
        }
        if mode == "pr" && !matches!(project["merge_method"].as_str(), Some("ff" | "rebase_merge")) {
            // main can move between the check above and the merge: only a fast-forward merge method refuses a stale head at merge time
            return Ok(Publish::Pending("project merge method does not require a rebased MR: set fast-forward or semi-linear".into()));
        }
        let iid = mr["iid"].as_u64().unwrap_or(0);
        if let Some(stop) = self.mergeable(iid)? {
            return Ok(stop);
        }
        let body = if mode == "pr" { json!({ "sha": sha, "squash": true, "squash_commit_message": format!("{title}\n\n{task_line}") }) } else { json!({ "sha": sha }) };
        let merged = match self.api.call("PUT", &format!("/merge_requests/{iid}/merge"), Some(body)) {
            // 422 'Branch cannot be merged' also answers a merge sent while GitLab rechecks: only a settled blocker fails (#1687)
            Err(e) if e.contains("HTTP 422") => return Ok(match self.merge_status(iid)? {
                Status::Blocked(why) => Publish::Failed(format!("!{iid} cannot be merged: {why}: {e}")),
                Status::Running(p) => self.running(&format!("!{iid} pipeline"), &p),
                _ => Publish::Pending(format!("!{iid} merge refused while GitLab checks mergeability: {e}")),
            }),
            r => r?,
        };
        let back = self.api.call("GET", &format!("/merge_requests/{iid}"), None)?.json()?;
        if back["state"] != "merged" {
            if let Ok(second) = self.api.call("GET", &format!("/merge_requests/{iid}"), None).and_then(|r| r.json())
                && second["state"] != "merged"
            {
                crate::incident::detect("publication.readback", concat!(file!(), ":", line!()), "none", &[mode], &["merge:unconfirmed"]);
            }
        }
        if back["state"] == "merged" {
            let merged = merged.json().ok();
            let target = if mode == "direct" { sha } else { merged.as_ref().and_then(|v| v["squash_commit_sha"].as_str()).unwrap_or("") };
            self.publication_ancestry(target, mode);
        }
        if back["state"] == "merged" { Ok(Publish::Done(format!("merged !{iid}"))) } else { Err(format!("!{iid} not merged yet")) }
    }
}

/// MR mergeability (§ 7). GitLab computes it asynchronously: after a push, or after another MR moves `main`, every open
/// MR reads `unchecked`/`checking` and `PUT merge` answers 422 'Branch cannot be merged' until the recheck ends (#1687).
enum Status {
    Mergeable,
    Checking(String),
    /// `ci_still_running`: the MR's head pipeline (#1939).
    Running(Value),
    Blocked(String),
}

/// Pipeline states that end by themselves; `manual` and `scheduled` wait for a person.
const RUNNING: [&str; 5] = ["created", "waiting_for_resource", "preparing", "pending", "running"];
/// A pipeline still running this long after it started needs the owner (§ 7).
pub(super) const PIPELINE_DEADLINE: i64 = 24 * 3600;

/// ` <pipeline url>` when the board names one.
fn link(p: &Value) -> String {
    p["web_url"].as_str().map_or(String::new(), |u| format!(" {u}"))
}

/// Reads of a checking MR before close gives up as Pending; the review retries the close later.
pub(super) const MERGE_STATUS_READS: u32 = 5;
#[cfg(not(test))]
const MERGE_STATUS_WAIT: std::time::Duration = std::time::Duration::from_secs(2);
#[cfg(test)]
const MERGE_STATUS_WAIT: std::time::Duration = std::time::Duration::ZERO;

impl GitLab {
    fn merge_status(&self, iid: u64) -> Result<Status> {
        // reading one MR also queues its recheck
        let v = self.api.call("GET", &format!("/merge_requests/{iid}"), None)?.json()?;
        // `detailed_merge_status` (GitLab 15.6+); legacy `merge_status` otherwise
        Ok(match (v["detailed_merge_status"].as_str(), v["merge_status"].as_str()) {
            (Some("mergeable"), _) | (None, Some("can_be_merged")) => Status::Mergeable,
            (Some("ci_still_running"), _) => Status::Running(v["head_pipeline"].clone()),
            (Some("ci_must_pass"), _) => Status::Blocked(format!("ci_must_pass{}", link(&v["head_pipeline"]))),
            (Some(st @ ("unchecked" | "checking" | "preparing" | "approvals_syncing")), _)
            | (None, Some(st @ ("unchecked" | "checking" | "cannot_be_merged_recheck" | "cannot_be_merged_rechecking"))) => Status::Checking(st.into()),
            (Some(st), _) | (None, Some(st)) => Status::Blocked(st.into()),
            (None, None) => Status::Checking("unknown merge status".into()),
        })
    }

    /// None when the MR is mergeable; else the outcome that stops this close.
    fn mergeable(&self, iid: u64) -> Result<Option<Publish>> {
        let mut last = String::new();
        for read in 0..MERGE_STATUS_READS {
            if read > 0 { std::thread::sleep(MERGE_STATUS_WAIT); }
            match self.merge_status(iid)? {
                Status::Mergeable => return Ok(None),
                Status::Blocked(why) => return Ok(Some(Publish::Failed(format!("!{iid} cannot be merged: {why}")))),
                Status::Running(p) => return Ok(Some(self.running(&format!("!{iid} pipeline"), &p))),
                Status::Checking(st) => last = st,
            }
        }
        Ok(Some(Publish::Pending(format!("!{iid} merge status {last}: GitLab still checks"))))
    }
}

impl Board for GitLab {
    fn canonical_labels(&self) -> Result<bool> {
        self.api.canonical_labels()
    }

    fn ensure_labels(&self, names: &[String]) -> Result<()> {
        self.api.ensure_labels(names, |c| format!("#{c}"), |current, payload| {
            let (method, path) = match current {
                Some(v) => ("PUT", format!("/labels/{}", v["id"].as_u64().ok_or("label has no id")?)),
                None => ("POST", "/labels".into()),
            };
            self.api.call(method, &path, Some(payload)).map(|_| ())
        })
    }

    fn issues(&self, since: Option<i64>) -> Result<Vec<Issue>> {
        let url = match since {
            Some(t) => format!("/issues?state=all&per_page=100&updated_after={}", stamp(t)),
            None => "/issues?state=opened&per_page=100".into(),
        };
        Ok(self.api.all(&url)?.iter().map(|v| self.parse_issue(v)).collect())
    }

    fn closed_tasks(&self, page: u32, since: Option<i64>) -> Result<(Vec<(Issue, i64)>, bool)> {
        let since = since.map_or(String::new(), |t| format!("&updated_after={}", stamp(t)));
        let r = self.api.call("GET", &format!("/issues?state=closed&labels={}&order_by=updated_at&sort=desc&per_page={}&page={}{since}", crate::service::TASK_LABEL, super::CLOSED_PAGE, page.max(1)), None)?;
        let list = r.json()?;
        let rows = list.as_array().ok_or("invalid board list: expected array")?;
        Ok((rows.iter().map(|v| (self.parse_issue(v), iso(&s(v, "closed_at")).unwrap_or(0))).collect(), r.next().is_some()))
    }

    fn issue(&self, n: u64) -> Result<Issue> {
        Ok(self.parse_issue(&self.api.call("GET", &format!("/issues/{n}"), None)?.json()?))
    }

    fn research_tasks(&self) -> Result<Vec<(Issue, i64)>> {
        let mut rows=std::collections::BTreeMap::new();
        for label in ["research", "docs"] {
            for v in self.api.all(&format!("/issues?state=closed&labels=taskq,{label}&per_page=100"))? {
                let i=self.parse_issue(&v); rows.insert(i.number,(i,iso(&s(&v,"closed_at")).unwrap_or(0)));
            }
        }
        Ok(rows.into_values().collect())
    }

    fn source(&self,path:&str,sha:&str)->Result<String> {
        use base64::{Engine,engine::general_purpose::STANDARD};
        let v=self.api.call("GET",&format!("/repository/files/{}?ref={}",super::encode(path),super::encode(sha)),None)?.json()?;
        if v["encoding"]!="base64" {return Err("report source encoding unknown".into());}
        let bytes=STANDARD.decode(s(&v,"content").replace(['\r','\n'],"")).map_err(|_|"report source encoding invalid")?;
        String::from_utf8(bytes).map_err(|_|"report source is not UTF-8".into())
    }

    fn comments(&self, n: u64) -> Result<Vec<Comment>> {
        Ok(self.api.all(&format!("/issues/{n}/notes?sort=asc&order_by=created_at&per_page=100"))?.iter()
            .filter(|v| v["system"].as_bool() != Some(true))
            .map(|v| Comment {
                id: v["id"].as_u64().unwrap_or(0),
                created: iso(&s(v, "created_at")).unwrap_or(0),
                body: s(v, "body"),
                trusted: self.member(v["author"]["id"].as_u64().unwrap_or(0)),
                edited: s(v, "updated_at") != s(v, "created_at"),
                author: s(&v["author"], "username"),
            }).collect())
    }

    fn comment(&self, n: u64, body: &str) -> Result<u64> {
        self.api.call("POST", &format!("/issues/{n}/notes"), Some(json!({ "body": body })))?.json()?["id"].as_u64().ok_or("no comment id".into())
    }

    fn edit_comment(&self, n: u64, id: u64, body: &str) -> Result<()> {
        self.api.call("PUT", &format!("/issues/{n}/notes/{id}"), Some(json!({ "body": body }))).map(|_| ())
    }

    fn labels(&self, n: u64, add: &[String], remove: &[String]) -> Result<()> {
        if !add.is_empty() && self.canonical_labels()? { self.ensure_labels(add)?; }
        self.api.call("PUT", &format!("/issues/{n}"), Some(json!({ "add_labels": csv(add), "remove_labels": csv(remove) })))?;
        verify_assignment(self, n, add, remove)
    }

    fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<u64> {
        if let Some(names) = self.api.creation_labels(labels)? { self.ensure_labels(&names)?; }
        let n = self.api.call("POST", "/issues", Some(json!({ "title": title, "description": body, "labels": csv(labels) })))?.json()?["iid"].as_u64().ok_or("no issue number")?;
        verify_assignment(self, n, labels, &[])?;
        Ok(n)
    }

    fn edit(&self, n: u64, body: &str) -> Result<()> {
        self.api.call("PUT", &format!("/issues/{n}"), Some(json!({ "description": body }))).map(|_| ())
    }

    fn close(&self, n: u64) -> Result<()> {
        self.api.call("PUT", &format!("/issues/{n}"), Some(json!({ "state_event": "close" }))).map(|_| ())
    }

    fn now(&self) -> Result<i64> {
        self.api.now(KIND.user_key)
    }

    fn me(&self) -> Result<String> {
        self.api.me(KIND.user_key)
    }

    fn publish(&self, n: u64, sha: &str, mode: &str, title: &str, task_line: &str) -> Publish {
        outcome(self.try_publish(n, sha, mode, title, task_line))
    }

    fn requests(&self) -> crate::requests::Budget {
        self.api.credentials.borrow().budget.borrow().clone()
    }

    fn url(&self, n: u64) -> String {
        self.foreign_url(&self.api.base.rsplit_once("/projects/").map(|x| x.1.replace("%2F", "/")).unwrap_or_default(), n)
    }

    fn foreign(&self, repo: &str, n: u64) -> Result<Issue> {
        let path = self.api.sibling("/projects/", repo).replace('/', "%2F");
        let v = super::dep_reply(self.api.call("GET", &format!("{}/projects/{path}/issues/{n}", self.api.root), None)?.json()?, "iid", n, "opened")?;
        // trust is for log entries; a dependency is judged only by its state
        Ok(Issue { number: v["iid"].as_u64().unwrap_or(0), open: v["state"] == "opened", ..Default::default() })
    }

    fn foreign_url(&self, repo: &str, n: u64) -> String {
        format!("{}/{}/-/issues/{n}", self.api.root.trim_end_matches("/api/v4"), self.api.sibling("/projects/", repo))
    }

    fn upload_file(&self, _n: u64, path: &std::path::Path) -> Result<String> {
        // the multipart part is named after the file: copy it under a plain ASCII name first
        // and a unique one: GitLab 17.2 replies without the upload id, which the newest uploads list finds by name
        let id = crate::runtime::uuid();
        let dir = std::env::temp_dir().join(format!("taskq-image-{id}"));
        std::fs::create_dir_all(&dir).map_err(|e| format!("image: {e}"))?;
        let name = format!("{}-{}", &id[..8], image_name(path));
        let file = dir.join(&name);
        let sent = std::fs::copy(path, &file).map_err(|e| format!("image {}: {e}", path.display()))
            .and_then(|_| self.api.raw("/uploads", Some(&file)));
        let _ = std::fs::remove_dir_all(&dir);
        let mut reply: Value = serde_json::from_slice(&sent?).map_err(|e| format!("bad JSON from board: {e}"))?;
        if reply["id"].is_null() {
            let list = self.api.call("GET", "/uploads?per_page=20", None)?.json()?;
            reply["id"] = list.as_array().into_iter().flatten().find(|u| u["filename"] == name.as_str()).map(|u| u["id"].clone()).unwrap_or_default();
        }
        upload_url(&self.api.root, self.project()?, &reply)
    }

    fn file(&self, url: &str) -> Result<Vec<u8>> {
        let unknown = || format!("image unknown: {url} is not a taskq upload of this project");
        let project = self.project()?;
        let id = match upload_id(&self.api.root, project, url) {
            Some(id) => id,
            // a video URL carries no `#upload=` id: the newest uploads find it by its unique file name
            None => {
                let name = video_name(&self.api.root, project, url).ok_or_else(unknown)?;
                let list = self.api.call("GET", "/uploads?per_page=100", None)?.json()?;
                list.as_array().into_iter().flatten().find(|u| u["filename"] == name).and_then(|u| u["id"].as_u64()).ok_or_else(unknown)?
            }
        };
        self.api.raw(&format!("/uploads/{id}"), None)
    }

    fn video_inline(&self) -> bool { true }
}

impl GitLab {
    /// The numeric project id upload URLs carry.
    fn project(&self) -> Result<u64> {
        self.api.call("GET", "", None)?.json()?["id"].as_u64().ok_or_else(|| "project id unknown".into())
    }
}

/// § 2 visual evidence (#1782): the absolute URL of a GitLab upload. The API's relative `url` and the `full_path` of
/// GitLab 17.2 render as broken repository paths in issue notes, so taskq writes
/// `https://<host>/-/project/<id>/uploads/<secret>/<file>` with `#upload=<upload id>`, the id the load check reads back;
/// a video (#1844) goes without the fragment and the load check finds its id by the unique file name.
pub(super) fn upload_url(root: &str, project: u64, reply: &Value) -> Result<String> {
    let rel = reply["url"].as_str().and_then(|u| u.strip_prefix("/uploads/")).filter(|r| r.split('/').count() == 2 && !r.contains(['(', ')', ' ']))
        .ok_or("upload reply has no /uploads/<secret>/<file> url")?;
    let id = reply["id"].as_u64().ok_or("upload id unknown: not in the reply nor the newest uploads; GitLab 17.2 or newer needed")?;
    let url = format!("{}/-/project/{project}/uploads/{rel}", root.trim_end_matches("/api/v4"));
    // #1844: GitLab plays `![name](url)` inline only when the URL ends in the video extension, so a video has no fragment
    Ok(if video(&url) { url } else { format!("{url}#upload={id}") })
}

fn video(url: &str) -> bool {
    [".mp4", ".webm"].iter().any(|e| url.to_ascii_lowercase().ends_with(e))
}

/// The file name of a video URL that [`upload_url`] wrote for this project; none for any other URL.
fn video_name<'a>(root: &str, project: u64, url: &'a str) -> Option<&'a str> {
    let rel = url.strip_prefix(&format!("{}/-/project/{project}/uploads/", root.trim_end_matches("/api/v4")))?;
    let (secret, name) = rel.split_once('/')?;
    (video(url) && !secret.is_empty() && !name.is_empty() && !name.contains(['/', '#', '?'])).then_some(name)
}

/// The upload id of an image URL that [`upload_url`] wrote for this project; none for any other URL.
pub(super) fn upload_id(root: &str, project: u64, url: &str) -> Option<u64> {
    let (path, id) = url.split_once("#upload=")?;
    path.strip_prefix(&format!("{}/-/project/{project}/uploads/", root.trim_end_matches("/api/v4")))?;
    id.parse().ok()
}

#[cfg(test)]
mod create_claim_tests {
    use super::*;
    use super::super::Reply;
    use std::{cell::RefCell, rc::Rc};

    /// A GitLab project whose issue 1 holds create claims; `store` turns a POSTed (title, description, labels) into
    /// what GitLab keeps.
    fn board(store: fn(&str, &str, Vec<String>) -> (String, String, Vec<String>)) -> Box<dyn Board> {
        let issues = Rc::new(RefCell::new(vec![(1u64, "Claims".to_string(), String::new(), vec![])]));
        let notes: Rc<RefCell<Vec<(u64, u64, String)>>> = Rc::default();
        let mut api = api("gitlab.example", "o/r", "test");
        *api.label_schema.borrow_mut() = Some(false);
        api.transport = Some(Box::new(move |method, url, body| {
            let at = "2026-10-30T00:00:00Z";
            let issue = |(n, title, body, labels): &(u64, String, String, Vec<String>)| json!({"iid": n, "title": title,
                "description": body, "labels": labels, "state": "opened", "author": {"id": 1}, "created_at": at, "updated_at": at});
            let path = url.split('?').next().unwrap().trim_start_matches("https://gitlab.example/api/v4");
            let rest = path.strip_prefix("/projects/o%2Fr/issues").map(|r| r.trim_start_matches('/'));
            let v = match (method, rest, rest.and_then(|r| r.split_once('/'))) {
                _ if path == "/user" => json!({"username": "owner"}),
                _ if path.starts_with("/projects/o%2Fr/members/all/") => json!({"access_level": 30}),
                ("GET", Some(""), _) => json!(issues.borrow().iter().map(issue).collect::<Vec<_>>()),
                ("POST", Some(""), _) => {
                    let body = body.unwrap();
                    let labels = body["labels"].as_str().unwrap().split(',').map(String::from).collect();
                    let (title, text, labels) = store(body["title"].as_str().unwrap(), body["description"].as_str().unwrap(), labels);
                    let n = issues.borrow().len() as u64 + 1;
                    issues.borrow_mut().push((n, title, text, labels));
                    json!({"iid": n})
                }
                ("GET", _, Some((n, "notes"))) => json!(notes.borrow().iter().filter(|(i, ..)| i.to_string() == n).map(|(_, id, body)|
                    json!({"id": id, "body": body, "system": false, "author": {"id": 1, "username": "owner"}, "created_at": at, "updated_at": at})).collect::<Vec<_>>()),
                ("POST", _, Some((n, "notes"))) => {
                    let id = 100 + notes.borrow().len() as u64;
                    notes.borrow_mut().push((n.parse().unwrap(), id, body.unwrap()["body"].as_str().unwrap().into()));
                    json!({"id": id})
                }
                ("GET", Some(n), None) => issue(issues.borrow().iter().find(|i| i.0.to_string() == n).unwrap()),
                _ => panic!("unexpected {method} {url}"),
            };
            Ok(Reply { status: 200, headers: vec![], body: v.to_string(), operation: "read" })
        }));
        (KIND.board)(api)
    }

    // Chain: § 1 `add --op-id` on GitLab -> create readback and retry discovery (#1914). Defect: gradus/cyprusbz#540,
    // GitLab keeps a description without the trailing newline `add` sends, so the first add reported 'create readback
    // does not match the request' and every retry 'created issue conflicts with the immutable request'. Risk: a
    // different title, body or label set is accepted as the requested issue.
    #[test]
    fn gitlab_add_op_id_accepts_its_stripped_description_and_still_refuses_other_content() {
        let raw = json!({"create_claim_issue": 1});
        let op = crate::Op { id: Some("pilotq-1.p18-t1".into()), ..Default::default() };
        let body = "## Goal\nG\n\n## Acceptance\nA1. Check\n\nop-id: pilotq-1.p18-t1\n";
        let labels = vec!["q-held".to_string()];
        // GitLab strips the surrounding whitespace of a stored title and description
        let b = board(|title, text, labels| (title.trim().into(), text.trim().into(), labels));
        let n = crate::claimed_create(b.as_ref(), &raw, "T", body, &labels, "", &op).unwrap();
        let r = crate::claimed_create(b.as_ref(), &raw, "T", body, &labels, "", &op).unwrap_err();
        assert_eq!((r.outcome, r.task, r.state), ("accepted", n, Some("held")), "{}", r.reason);
        let cases: [(&str, fn(&str, &str, Vec<String>) -> (String, String, Vec<String>)); 3] = [
            ("title", |_, text, labels| ("Other".into(), text.into(), labels)),
            ("body", |title, text, labels| (title.into(), text.replace("A1. Check", "A1. Other"), labels)),
            ("labels", |title, text, labels| (title.into(), text.into(), [labels, vec!["bug".into()]].concat())),
        ];
        for (field, store) in cases {
            let b = board(store);
            let r = crate::claimed_create(b.as_ref(), &raw, "T", body, &labels, "", &op).unwrap_err();
            assert_eq!((r.outcome, r.reason.as_str()), ("unknown", "create readback does not match the request"), "{field}");
            // labels move with the task state after start, so retry discovery identifies the issue by title and body
            let r = crate::claimed_create(b.as_ref(), &raw, "T", body, &labels, "", &op).unwrap_err();
            let want = if field == "labels" { ("accepted", "") } else { ("unknown", "created issue conflicts with the immutable request") };
            assert_eq!((r.outcome, r.reason.as_str()), want, "{field}");
        }
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;

    // Chain: § 9 GitLab token lookup -> CLI error (#1894). Defect: with no `<install>/gitlab-token` and no `glab`, every
    // board read failed with `board token lookup unavailable: process spawn: ...`, naming neither. Risk: the error loses
    // the file, `glab` or the Next line, or rewrites another lookup error.
    #[test]
    fn missing_file_and_glab_name_both_with_next() {
        let file = std::path::Path::new("/invented/install/gitlab-token");
        let e = missing(file, "gitlab.example", "board token lookup unavailable: process spawn: No such file or directory (os error 2)");
        let (error, next) = e.split_once('\n').unwrap();
        assert_eq!(error, format!("no GitLab token: no {} and `glab` cannot run (process spawn: No such file or directory (os error 2))", file.display()));
        assert_eq!(next, "Next: write a GitLab access token for gitlab.example to that file, or install glab and log in to gitlab.example");
        assert_eq!(missing(file, "h", "no board token: set X_TOKEN or write f"), "no board token: set X_TOKEN or write f");
        // End to end only where the host has no runnable `glab` and no env token.
        let glab = std::process::Command::new("glab").arg("--version").output().is_ok();
        if glab || std::env::var_os("GITLAB_TOKEN").is_some() { return; }
        let install = std::env::temp_dir().join(format!("taskq-gitlab-missing-{}", std::process::id()));
        std::fs::create_dir_all(&install).unwrap();
        let tool = coreq::Tool { install: install.clone(), ..coreq::Tool::new("taskqgitlabmissingtest", "TaskQ", "fixture/unused", "1.2.3") };
        let e = token(&tool, "gitlab.example").unwrap_err();
        assert!(e.starts_with(&format!("no GitLab token: no {} and `glab` cannot run (", install.join("gitlab-token").display())), "{e}");
        assert!(e.ends_with("\nNext: write a GitLab access token for gitlab.example to that file, or install glab and log in to gitlab.example"), "{e}");
        std::fs::remove_dir_all(install).unwrap();
    }

    // Chain: § 2 visual evidence on GitLab (#1782). Defect: csgo#378 embedded the upload API's relative url (and
    // GitLab 17.2 full_path), which issue notes rewrite into broken repository paths. Risk: a relative or foreign URL is
    // written or load-checked as if it were this project's upload.
    #[test]
    fn gitlab_uploads_embed_absolute_urls_and_only_own_uploads_load() {
        let root = "https://gitlab.example/api/v4";
        let reply = json!({"id": 214, "url": "/uploads/cef859da/front.png", "full_path": "/alex/csgo/uploads/cef859da/front.png", "markdown": "![front](/uploads/cef859da/front.png)"});
        let url = upload_url(root, 39, &reply).unwrap();
        assert_eq!(url, "https://gitlab.example/-/project/39/uploads/cef859da/front.png#upload=214");
        assert_eq!(upload_id(root, 39, &url), Some(214));
        assert_eq!(upload_id(root, 40, &url), None, "another project's upload");
        assert_eq!(upload_id(root, 39, "/uploads/cef859da/front.png"), None, "relative link");
        assert_eq!(upload_id(root, 39, "https://gitlab.example/alex/csgo/uploads/cef859da/front.png"), None, "full_path link");
        assert!(upload_url(root, 39, &json!({"url": "/uploads/x/a.png"})).is_err(), "no id: no load check possible");
        assert!(upload_url(root, 39, &json!({"id": 1, "full_path": "/alex/csgo/uploads/x/a.png"})).is_err());
    }

    // Chain: § 2 visual evidence videos on GitLab (#1844). Defect: csgo#415 hand-posted host-less video links that GitLab
    // rewrote into broken repository paths. Risk: a video URL keeps a fragment that stops inline play, or a foreign or
    // relative URL is load-checked as this project's upload.
    #[test]
    fn gitlab_videos_embed_absolute_urls_ending_in_their_extension() {
        let root = "https://gitlab.example/api/v4";
        let url = upload_url(root, 39, &json!({"id": 215, "url": "/uploads/cef859da/abcd1234-run.mp4"})).unwrap();
        assert_eq!(url, "https://gitlab.example/-/project/39/uploads/cef859da/abcd1234-run.mp4");
        assert_eq!(video_name(root, 39, &url), Some("abcd1234-run.mp4"));
        assert_eq!(upload_url(root, 39, &json!({"id": 216, "url": "/uploads/ab/clip.webm"})).unwrap(), "https://gitlab.example/-/project/39/uploads/ab/clip.webm");
        assert_eq!(video_name(root, 40, &url), None, "another project's upload");
        assert_eq!(video_name(root, 39, "/-/project/39/uploads/cef859da/abcd1234-run.mp4"), None, "host-less link");
        assert_eq!(video_name(root, 39, "https://gitlab.example/-/project/39/uploads/ab/front.png"), None, "an image keeps its id");
    }
}
