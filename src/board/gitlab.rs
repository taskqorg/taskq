//! GitLab issues (taskq.md § 1, § 2, § 7): REST v4 on gitlab.com or a self-managed host, `glab` login.

use super::{Api, Board, image_name, Comment, Issue, Kind, Publish, Result, hex40, into, iso, names, outcome, s, stamp, verify_assignment};
use serde_json::{Value, json};
use std::cell::RefCell;

pub const KIND: Kind = Kind {
    reload_token: false,
    name: "gitlab",
    user_key: "username",
    default_host: "gitlab.com",
    token: |tool, host| tool.token(true, host),
    token_env: &["GITLAB_TOKEN"],
    api,
    board: |api| Box::new(GitLab { api, members: RefCell::new(vec![]) }),
};

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

    /// CI on `sha`: Ok(None) green, Ok(Some(msg)) failed, Err pending or missing.
    fn ci(&self, sha: &str) -> Result<Option<String>> {
        let v = self.api.call("GET", &format!("/repository/commits/{sha}"), None)?.json()?;
        match v["last_pipeline"]["status"].as_str() {
            Some("success") => Ok(None),
            Some(st @ ("failed" | "canceled")) => Ok(Some(format!("CI {st} on {sha}"))),
            Some(st) => Err(format!("CI {st} on {sha}")),
            None => Err(format!("no CI on {sha}")),
        }
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
        if let Some(fail) = self.ci(sha)? {
            return Ok(Publish::Failed(fail));
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
        if mode == "pr" && !matches!(self.api.call("GET", "", None)?.json()?["merge_method"].as_str(), Some("ff" | "rebase_merge")) {
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
    Blocked(String),
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
            (Some(st @ ("unchecked" | "checking" | "preparing" | "approvals_syncing" | "ci_still_running")), _)
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
mod image_tests {
    use super::*;

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
