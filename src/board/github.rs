//! GitHub issues (taskq.md § 1, § 2, § 7): REST v3 on github.com or a GitHub Enterprise host, owner-installed token.

use super::{Api, Board, image_name, Comment, Issue, Kind, Publish, Result, encode, hex40, into, iso, names, outcome, s, stamp, verify_assignment};
use serde_json::{Value, json};

pub const KIND: Kind = Kind {
    closed_reason: true,
    name: "github",
    user_key: "login",
    default_host: "github.com",
    token: |tool, _host| token(&tool.install),
    reload_token: true,
    token_env: &["GH_TOKEN", "GITHUB_TOKEN"],
    api,
    board: |api| Box::new(GitHub { api, rules: Default::default(), open: Default::default() }),
};

pub const TOKEN_ERROR: &str = "GitHub board token missing, empty or unreadable; owner must install it in <install>/token; never run gh auth login";

pub fn token(install: &std::path::Path) -> Result<String> {
    std::fs::read_to_string(install.join("token")).ok()
        .map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).ok_or_else(|| TOKEN_ERROR.into())
}

fn api(host: &str, repo: &str, token: &str) -> Api {
    let root = if host == "github.com" || host.is_empty() { "https://api.github.com".to_string() } else { format!("https://{host}/api/v3") };
    Api::new(root.clone(), format!("{root}/repos/{repo}"), token)
}

/// The repository plan refuses branch rules: GitHub answers 403 "Upgrade to GitHub Pro" on a private Free repository.
fn plan_gap(e: &str) -> bool { e.contains("HTTP 403") && e.contains("Upgrade to GitHub Pro") }

const PLAN: &str = "this private repository's GitHub plan has no branch rules or merge queue";

pub struct GitHub {
    pub(super) api: Api,
    /// `main` branch rules, read once per credential (#1359), a plan refusal too (#1935). ponytail: same-token rules changes need a service restart.
    /// Keyed by repository API too: a task PR in another repository reads that repository's rules (#2047).
    rules: std::cell::RefCell<Option<(u64, String, Result<Value>)>>,
    /// The last open-issue list, reused while the changed-since probe finds nothing new (#1819, #1968).
    open: std::cell::RefCell<Option<Open>>,
}

struct Open {
    at: std::time::Instant,
    list: Vec<Issue>,
    /// `updated` of every issue the list accounts for, closed ones the relisting probe saw too; its maximum is the probe's `since`.
    known: std::collections::HashMap<u64, i64>,
}

/// The longest an open-issue list is reused over unchanged probes; a deleted issue does not bump `updated_at`.
const OPEN_REUSE: std::time::Duration = std::time::Duration::from_secs(300);

fn trusted(v: &Value) -> bool {
    matches!(v["author_association"].as_str(), Some("OWNER" | "MEMBER" | "COLLABORATOR"))
}

/// One issue comment of the REST API.
pub(crate) fn parse_comment(v: &Value) -> Comment {
    Comment {
        id: v["id"].as_u64().unwrap_or(0),
        created: iso(&s(v, "created_at")).unwrap_or(0),
        body: s(v, "body"),
        trusted: trusted(v),
        edited: s(v, "updated_at") != s(v, "created_at"),
        author: s(&v["user"], "login"),
    }
}

fn parse_issue(v: &Value) -> Issue {
    Issue {
        number: v["number"].as_u64().unwrap_or(0),
        title: s(v, "title"),
        body: s(v, "body"),
        labels: names(&v["labels"], "name"),
        assignees: names(&v["assignees"], "login"),
        updated: iso(&s(v, "updated_at")).unwrap_or(0),
        created: iso(&s(v, "created_at")).unwrap_or(0),
        closed: iso(&s(v, "closed_at")),
        open: v["state"] == "open",
        trusted: trusted(v),
        reason: v["state_reason"].as_str().map(String::from),
    }
}

/// The open-issue list of a pass in one GraphQL query per 100 issues (§ 4 budget): it costs the separate GraphQL
/// budget 1 point instead of one core request, and returns no pull requests. Labels are its only nested connection:
/// assignees would make it 2 points (#1043), so listed issues carry none and callers read `issue(n)` when they need them.
const OPEN_ISSUES: &str = "query($owner: String!, $name: String!, $after: String) { repository(owner: $owner, name: $name) { \
    issues(states: OPEN, first: 100, after: $after, orderBy: {field: CREATED_AT, direction: DESC}) { pageInfo { hasNextPage endCursor } \
    nodes { number title body updatedAt createdAt authorAssociation labels(first: 50) { nodes { name } } } } } }";

impl GitHub {
    /// Every board change bumps some issue's `updated_at`; one `since` query of the newest known update finds them,
    /// a 304 when nothing changed (#1819). Heartbeat writes and pull requests alone patch the list instead of relisting
    /// it (#1968): three hosts beating every minute would otherwise relist on every pass.
    fn open_unchanged(&self) -> Result<Vec<Issue>> {
        let cached = self.open.borrow_mut().take().filter(|o| o.at.elapsed() < OPEN_REUSE);
        let mut seen = vec![];
        if let Some(mut o) = cached {
            let since = o.known.values().max().copied().unwrap_or(0);
            let url = format!("/issues?state=all&sort=updated&direction=desc&per_page=100&since={}", stamp(since));
            if let Ok(Value::Array(rows)) = self.api.call("GET", &url, None).and_then(|r| r.json()) {
                let issues: Vec<Issue> = rows.iter().filter(|v| v.get("pull_request").is_none()).map(parse_issue).collect();
                if rows.len() < 100 && issues.iter().all(|i| crate::service::is_host(i) || o.known.get(&i.number) == Some(&i.updated)) {
                    for h in issues.into_iter().filter(crate::service::is_host) {
                        match o.list.iter().position(|i| i.number == h.number) {
                            Some(at) if h.open => o.list[at] = h,
                            Some(at) => { o.list.remove(at); }
                            None if h.open => o.list.push(h),
                            None => {}
                        }
                    }
                    let list = o.list.clone();
                    *self.open.borrow_mut() = Some(o);
                    return Ok(list);
                }
                seen = issues;
            }
        }
        // a failed list keeps nothing: the next pass reads in full
        let list = self.open_issues()?;
        let known = seen.iter().chain(&list).map(|i| (i.number, i.updated)).collect();
        *self.open.borrow_mut() = Some(Open { at: std::time::Instant::now(), list: list.clone(), known });
        Ok(list)
    }

    fn open_issues(&self) -> Result<Vec<Issue>> {
        let repo = self.api.base.rsplit_once("/repos/").map(|x| x.1).unwrap_or_default();
        let (owner, name) = repo.split_once('/').ok_or("no repository")?;
        let (mut out, mut after) = (vec![], Value::Null);
        loop {
            let v = self.api.call("POST", &format!("{}/graphql", self.api.root), Some(json!({
                "query": OPEN_ISSUES, "variables": { "owner": owner, "name": name, "after": after },
            })))?.json()?;
            let page = &v["data"]["repository"]["issues"];
            if v["errors"].as_array().is_some_and(|errors| errors.iter().any(|e| e["type"] == "NOT_FOUND")) {
                return Err("no access to the repository".into());
            }
            if !page["nodes"].is_array() {
                return Err(format!("GraphQL open issues: {}", v["errors"]));
            }
            out.extend(page["nodes"].as_array().into_iter().flatten().map(|n| parse_issue(&json!({
                "number": n["number"], "title": n["title"], "body": n["body"], "updated_at": n["updatedAt"], "created_at": n["createdAt"], "state": "open",
                "author_association": n["authorAssociation"], "labels": n["labels"]["nodes"],
            }))));
            if page["pageInfo"]["hasNextPage"] != true { return Ok(out); }
            after = page["pageInfo"]["endCursor"].clone();
        }
    }

    /// CI on `sha`: Ok(None) green, Ok(Some(msg)) failed, Err pending or missing.
    fn ci(&self, sha: &str) -> Result<Option<String>> {
        self.ci_where(sha, &|_| true)
    }

    /// `ci` over the check runs `keep` accepts; commit statuses always count.
    fn ci_where(&self, sha: &str, keep: &dyn Fn(&Value) -> bool) -> Result<Option<String>> {
        let runs = self.api.call("GET", &format!("/commits/{sha}/check-runs?per_page=100"), None)?.json()?;
        let runs: Vec<Value> = runs["check_runs"].as_array().into_iter().flatten().filter(|r| keep(r)).cloned().collect();
        let status = self.api.call("GET", &format!("/commits/{sha}/status"), None)?.json()?;
        let statuses = status["total_count"].as_u64().unwrap_or(0);
        if runs.is_empty() && statuses == 0 {
            return Err(format!("no CI on {sha}"));
        }
        if let Some(r) = runs.iter().find(|r| r["status"] != "completed") {
            return Err(format!("CI {} pending on {sha}", s(r, "name")));
        }
        if let Some(r) = runs.iter().find(|r| !matches!(r["conclusion"].as_str(), Some("success" | "neutral" | "skipped"))) {
            return Ok(Some(format!("CI {} {} on {sha}", s(r, "name"), s(r, "conclusion"))));
        }
        match status["state"].as_str() {
            _ if statuses == 0 => Ok(None),
            Some("success") => Ok(None),
            Some("pending") => Err(format!("CI pending on {sha}")),
            st => Ok(Some(format!("CI {} on {sha}", st.unwrap_or("unknown")))),
        }
    }

    fn publication_ancestry(&self, sha: &str, mode: &str) {
        // Only the immediate read in this close qualifies; later main changes are not incidents.
        if !hex40(sha) { return; }
        let on_main = self.api.call("GET", &format!("/compare/main...{sha}"), None).and_then(|r| r.json())
            .and_then(|v| match v["status"].as_str() {
                Some("identical" | "behind") => Ok(true), Some("ahead" | "diverged") => Ok(false), _ => Err("unknown ancestry".into()),
            });
        if on_main == Ok(false) {
            crate::incident::detect("publication.not_on_main", concat!(file!(), ":", line!()), "none", &[mode], &["ancestry:missing"]);
        }
    }

    /// main refuses a merge of a head that lacks current main and this token cannot bypass it (§ 7):
    /// a ruleset it can never bypass, or classic protection the token can read that binds admins too.
    fn strict_main(&self) -> bool {
        let read = |url: &str| self.api.call("GET", url, None).and_then(|r| r.json());
        self.main_rules().is_ok_and(|v| v.as_array().into_iter().flatten()
            .filter(|r| r["type"] == "required_status_checks" && r["parameters"]["strict_required_status_checks_policy"] == true)
            .any(|r| r["ruleset_id"].as_u64().is_some_and(|id| read(&format!("/rulesets/{id}")).is_ok_and(|s| s["current_user_can_bypass"] == "never"))))
            || read("/branches/main/protection").is_ok_and(|v| v["required_status_checks"]["strict"] == true && v["enforce_admins"]["enabled"] == true)
    }

    fn main_rules(&self) -> Result<Value> {
        self.api.refresh()?;
        let repo = self.api.repo();
        if let Some((_, _, v)) = self.rules.borrow().as_ref().filter(|(g, r, _)| *g == self.api.generation.get() && *r == repo) { return v.clone(); }
        let v = self.api.call("GET", "/rules/branches/main", None).and_then(|r| r.json());
        // #1935: a plan without branch rules refuses every read; probe it once, other failures retry next pass
        if v.as_ref().is_ok() || v.as_ref().is_err_and(|e| plan_gap(e)) { *self.rules.borrow_mut() = Some((self.api.generation.get(), repo, v.clone())); }
        v
    }

    /// Remove PR `id` from the merge queue with readback; a merged or unreadable PR is an error (§ 7).
    fn dequeue(&self, id: &str) -> Result<()> {
        let query = "query($id: ID!) { node(id: $id) { ... on PullRequest { merged mergeQueueEntry { id } } } }";
        let read = || self.graphql(query, json!({"id": id}));
        let v = read()?;
        if v["node"]["merged"] != false { return Err("PR already merged or unknown; verify close before cancelling".into()); }
        if v["node"]["mergeQueueEntry"].is_object() {
            self.graphql("mutation($id: ID!) { dequeuePullRequest(input: {id: $id}) { clientMutationId } }", json!({"id": id}))?;
            let back = read()?;
            if back["node"]["merged"] != false || back["node"].get("mergeQueueEntry") != Some(&Value::Null) { return Err("queue cancellation not read back; task unchanged".into()); }
        } else if v["node"].get("mergeQueueEntry") != Some(&Value::Null) { return Err("queue cancellation state unknown".into()); }
        Ok(())
    }

    fn graphql(&self, query: &str, variables: Value) -> Result<Value> {
        let v = self.api.call("POST", &format!("{}/graphql", self.api.root), Some(json!({"query": query, "variables": variables})))?.json()?;
        if let Some(e) = v["errors"].as_array().filter(|e| !e.is_empty()) {
            // #1820: type and message become a surfaced publication reason; raw JSON hid it.
            return Err(format!("publication GraphQL: {}", e.iter().map(|e| format!("{} {}", s(e, "type"), s(e, "message")).trim().to_string()).collect::<Vec<_>>().join("; ")));
        }
        if !v["data"].is_object() { return Err("publication GraphQL data unknown".into()); }
        Ok(v["data"].clone())
    }

    /// § 7: qualify the merge result, never substitute green checks on the original head.
    /// Err: pending or unknown; `Ok(Err)`: the merge result's CI failed; `Ok(Ok)`: green, plus a note on ignored push failures.
    fn merge_ci(&self, sha: &str) -> Result<Result<String>> {
        if !hex40(sha) { return Err("merge result SHA unknown".into()); }
        let v = self.api.call("GET", &format!("/actions/runs?event=merge_group&head_sha={sha}&per_page=100"), None)?.json()?;
        let runs = v["workflow_runs"].as_array().ok_or("merge_group CI unknown")?;
        let group: Vec<&Value> = runs.iter().filter(|r| r["head_sha"] == sha && r["event"] == "merge_group").collect();
        if !group.iter().any(|r| r["status"] == "completed" && r["conclusion"] == "success") {
            if let Some(r) = group.iter().find(|r| r["status"] == "completed") {
                return Ok(Err(format!("merge_group CI {} {} on merge result {sha}", s(r, "name"), s(r, "conclusion"))));
            }
            return Err(format!("merge_group CI not green on merge result {sha}"));
        }
        // #1343: the post-merge push runs on main retest the same tree and held close ~7 minutes; only merge_group Actions suites count.
        // #1413: a failed push run never holds close; the close receipt names it.
        let suites: Vec<&Value> = group.iter().map(|r| &r["check_suite_id"]).collect();
        let ignored = std::cell::RefCell::new(vec![]);
        let keep = |c: &Value| {
            let kept = c["app"]["slug"] != "github-actions" || suites.contains(&&c["check_suite"]["id"]);
            if !kept && c["status"] == "completed" && !matches!(c["conclusion"].as_str(), Some("success" | "neutral" | "skipped")) {
                ignored.borrow_mut().push(format!("{} {}", s(c, "name"), s(c, "conclusion")));
            }
            kept
        };
        if let Some(fail) = self.ci_where(sha, &keep)? { return Ok(Err(fail)); }
        let ignored = ignored.into_inner();
        Ok(Ok(if ignored.is_empty() { String::new() } else { format!("; push CI {} ignored", ignored.join(", ")) }))
    }

    /// #1820: the files main and the PR both changed since their merge base, read Git-free (a public board has no worktree).
    fn conflict_files(&self, sha: &str) -> String {
        let files = |v: &Value| v["files"].as_array().into_iter().flatten().filter_map(|f| f["filename"].as_str().map(String::from)).collect::<Vec<_>>();
        let read = |range: &str| self.api.call("GET", &format!("/compare/{range}"), None).and_then(|r| r.json());
        let Ok(ours) = read(&format!("main...{sha}")) else { return "conflict files unknown".into() };
        let Some(base) = ours["merge_base_commit"]["sha"].as_str().filter(|b| hex40(b)) else { return "conflict files unknown".into() };
        let Ok(theirs) = read(&format!("{base}...main")) else { return "conflict files unknown".into() };
        let theirs = files(&theirs);
        let both: Vec<String> = files(&ours).into_iter().filter(|f| theirs.contains(f)).collect();
        match both.len() {
            0 => "conflict files unknown".into(),
            n if n <= 5 => format!("conflict files: {}", both.join(", ")),
            n => format!("conflict files: {} +{} more", both[..5].join(", "), n - 5),
        }
    }

    fn queue(&self, pr: &Value, sha: &str, enqueued: bool) -> Result<Publish> {
        let id = pr["node_id"].as_str().ok_or("PR node id unknown")?;
        let number = &pr["number"];
        // #1820 (pilotq #106): a conflicting PR is never admitted; requeue in this pass, naming the files, instead of waiting silently.
        let conflict = || Publish::MainMoved(format!("PR #{number} conflicts with main; {}", self.conflict_files(sha)));
        let v = self.graphql("query($id: ID!) { node(id: $id) { ... on PullRequest { headRefOid mergeable merged mergeCommit { oid } mergeQueueEntry { id } timelineItems(last: 1, itemTypes: [REMOVED_FROM_MERGE_QUEUE_EVENT]) { nodes { ... on RemovedFromMergeQueueEvent { reason } } } } } }", json!({"id": id}))?;
        let p = &v["node"];
        let head = p["headRefOid"].as_str().ok_or("queued PR head unknown")?;
        if head != sha { return Ok(head_moved(number, head, sha)); }
        if p.get("mergeQueueEntry").is_none() { return Err("queue entry state unknown".into()); }
        if p["merged"] == true {
            let merged = p["mergeCommit"]["oid"].as_str().ok_or("merge result unknown")?;
            let note = match self.merge_ci(merged)? {
                Err(fail) => return Ok(Publish::Blocked(format!("PR #{} merged as {merged}, {fail}", pr["number"]))),
                Ok(note) => note,
            };
            self.publication_ancestry(merged, "pr");
            return Ok(Publish::Done(format!("merged #{} as {merged}; merge-result CI green{note}", pr["number"])));
        }
        if p["merged"] != false { return Err("queue merge state unknown".into()); }
        if let Some(id) = p["mergeQueueEntry"]["id"].as_str() { return Ok(Publish::Enqueued(id.into())); }
        if enqueued {
            let reason = p["timelineItems"]["nodes"][0]["reason"].as_str().ok_or("queue entry missing; removal reason unknown")?;
            let lower = reason.to_lowercase();
            // #1489: GitHub records the merge removal before the merged flag; reread next pass, never ask.
            if lower.contains("merged") { return Err(format!("merge queue removed PR as {reason}; merged state not read back yet")); }
            if lower.contains("failed") || lower.contains("failure") { return Ok(Publish::Failed(format!("merge queue removed PR: {reason}"))); }
            if !["conflict", "base branch", "out of date"].iter().any(|s| lower.contains(s)) { return Ok(Publish::Blocked(format!("merge queue removed PR: {reason}"))); }
            // #1820: a conflict removal is final only while the PR still conflicts; a clean PR (the conflicting entry ahead left) re-enqueues below.
            match p["mergeable"].as_str() {
                Some("MERGEABLE") => {},
                Some("CONFLICTING") => return Ok(conflict()),
                _ => return Err(format!("merge queue removed PR #{number} as {reason}; mergeability not computed yet")),
            }
        }
        if p["mergeable"] == "CONFLICTING" { return Ok(conflict()); }
        if pr["draft"] == true {
            let v = self.graphql("mutation($id: ID!) { markPullRequestReadyForReview(input: {pullRequestId: $id}) { pullRequest { isDraft } } }", json!({"id": id}))?;
            if v["markPullRequestReadyForReview"]["pullRequest"]["isDraft"] != false { return Err("PR is still a draft".into()); }
        }
        // #1372: GitHub refuses to enqueue a conflicting head; only the worker can fix it, so retrying each pass never helps.
        match self.graphql("mutation($id: ID!, $sha: GitObjectID!) { enqueuePullRequest(input: {pullRequestId: $id, expectedHeadOid: $sha}) { mergeQueueEntry { id } } }", json!({"id": id, "sha": sha})) {
            Err(e) if e.contains("UNPROCESSABLE") && e.contains("Pull request has merge conflicts") => return Ok(conflict()),
            // #1820: any other refusal (a required check still running) waits with its reason; a failed head check requeues instead of waiting forever.
            Err(e) if e.contains("UNPROCESSABLE") && !e.contains("already in the queue") => {
                let refused = format!("merge queue did not admit PR #{number}: {}", e.trim_start_matches("publication GraphQL: UNPROCESSABLE "));
                return match self.ci(sha) {
                    Ok(Some(fail)) => Ok(Publish::Failed(format!("{refused}; {fail}"))),
                    Ok(None) => Err(refused),
                    Err(wait) => Err(format!("{refused}; {wait}")),
                };
            }
            r => { r?; }
        }
        // Read back admission: a mutation response alone is not proof (R12); a retry recovers it after a lost response.
        let v = self.graphql("query($id: ID!) { node(id: $id) { ... on PullRequest { mergeQueueEntry { id } } } }", json!({"id": id}))?;
        let id = v["node"]["mergeQueueEntry"]["id"].as_str().ok_or("queue admission not yet read back")?;
        Ok(Publish::Enqueued(id.into()))
    }

    fn rebase(&self, root: &std::path::Path, n: u64, sha: &str) -> Result<Publish> {
        self.api.refresh()?;
        let path = root.join(".worktrees").join(format!("taskq-{n}"));
        let path = path.canonicalize().map_err(|_| "private fallback needs its owned task worktree on this host")?;
        if !path.starts_with(root.canonicalize().map_err(|_| "private fallback root unknown")?) {
            return Err("private fallback worktree escapes this project".into());
        }
        let host = self.api.root.strip_prefix("https://").unwrap_or_default().split('/').next().unwrap_or_default();
        let host = if host == "api.github.com" { "github.com" } else { host };
        let git = |args: &[&str]| -> Result<std::process::Output> {
            crate::platform::command("git").current_dir(&path).args(args).env("GIT_TERMINAL_PROMPT", "0")
                // Git's credential protocol receives the host board token through the environment, never argv/logs.
                .env("GIT_CONFIG_COUNT", "2").env("GIT_CONFIG_KEY_0", "credential.helper").env("GIT_CONFIG_VALUE_0", "")
                .env("GIT_CONFIG_KEY_1", format!("credential.https://{host}.helper"))
                .env("GIT_CONFIG_VALUE_1", "!f() { printf 'username=x-access-token\\npassword=%s\\n' \"$TASKQ_GIT_TOKEN\"; }; f")
                .env("TASKQ_GIT_TOKEN", self.api.credentials.borrow().auth.strip_prefix("Authorization: Bearer ").unwrap_or_default())
                .output().map_err(|_| "private fallback cannot run Git".into())
        };
        let read = |args: &[&str]| -> Result<String> {
            let out = git(args)?;
            if !out.status.success() { return Err(format!("private fallback `git {}` failed: {}: {}; worktree kept", args.join(" "), out.status, String::from_utf8_lossy(&out.stderr).trim())); }
            String::from_utf8(out.stdout).map(|s| s.trim().to_string()).map_err(|_| "private fallback Git output unknown".into())
        };
        let common = read(&["rev-parse", "--git-common-dir"])?;
        let common = path.join(common).canonicalize().map_err(|_| "private fallback ownership unknown")?;
        let project = crate::platform::command("git").current_dir(root).args(["rev-parse", "--git-common-dir"]).output().map_err(|e| format!("private fallback project unknown: `git rev-parse --git-common-dir` failed: {e}"))?;
        if !project.status.success() || root.join(String::from_utf8_lossy(&project.stdout).trim()).canonicalize().ok().as_ref() != Some(&common) {
            return Err("private fallback worktree belongs to another repository".into());
        }
        let expected = format!("taskq-{n}");
        if read(&["symbolic-ref", "--short", "HEAD"])? != expected || read(&["rev-parse", "HEAD"])? != sha || !read(&["status", "--porcelain"])?.is_empty() {
            return Err("private fallback refuses a dirty, changed or unowned task worktree".into());
        }
        let repo = self.api.base.rsplit_once("/repos/").map(|x| x.1).ok_or("repository unknown")?;
        let origin = read(&["config", "--get", "remote.origin.url"])?;
        let origin = origin.trim_end_matches('/').trim_end_matches(".git");
        if origin != format!("https://{host}/{repo}") && origin != format!("git@{host}:{repo}") {
            return Err("private fallback origin is not this board; worktree kept".into());
        }
        if !git(&["fetch", "origin", "main"])?.status.success() { return Err("private fallback fetch failed; worktree kept".into()); }
        if !git(&["rebase", "origin/main"])?.status.success() {
            let conflict = !read(&["ls-files", "-u"])?.is_empty();
            if !git(&["rebase", "--abort"])?.status.success() { return Err("private fallback rebase abort unknown; worktree kept".into()); }
            return if conflict { Ok(Publish::MainMoved("private fallback rebase conflict; worker must resolve against main".into())) }
                else { Err("private fallback rebase failed; worktree restored".into()) };
        }
        let head = read(&["rev-parse", "HEAD"])?;
        if !hex40(&head) { return Err("private fallback head unknown".into()); }
        if !git(&["push", "origin", &format!("HEAD:refs/heads/{expected}"), &format!("--force-with-lease=refs/heads/{expected}:{sha}")])?.status.success() {
            return Err("private fallback push failed; rebased worktree kept, publication unknown".into());
        }
        let back = self.api.call("GET", &format!("/git/ref/heads/{expected}"), None)?.json()?;
        if back["object"]["sha"] != head { return Err("private fallback pushed head not read back".into()); }
        Ok(Publish::Rebased(head))
    }

    fn try_publish(&self, n: u64, sha: &str, mode: &str, title: &str, task_line: &str, root: Option<&std::path::Path>, enqueued: bool) -> Result<Publish> {
        // #2061 (#2010 pilotq#200, #2015 dashq#241): a task PR of another repository publishes there, before any board
        // repository read; a named PR that is not this task's (head `taskq-<N>` or the result) is only a mention
        let mut unread = None;
        for (repo, p) in self.foreign_prs(n)? {
            match self.api.call("GET", &format!("{}/repos/{repo}/pulls/{p}", self.api.root), None).and_then(|r| r.json()) {
                Ok(pr) if pr["head"]["ref"] == format!("taskq-{n}") || pr["head"]["sha"] == sha => {
                    return self.publish_foreign(&repo, pr, (n, sha, mode, title, task_line), root, enqueued);
                }
                Ok(_) => {},
                Err(e) if unreachable(&e) => { unread.get_or_insert((format!("{repo}#{p}"), e)); },
                Err(e) => return Err(e),
            }
        }
        let status = match self.api.call("GET", &format!("/compare/main...{sha}"), None) {
            // #2047: the result is in no repository this board reads; an unreadable named PR blocks by name at once
            Err(e) if e.contains("HTTP 404") => return match unread {
                Some((name, e)) => Ok(Publish::Blocked(format!("publication PR {name} unreachable: {e}"))),
                // #2061 A4: one stable reason per result, so the service logs it once, naming the repository
                None => Err(format!("result {sha} not found in {} (compare HTTP 404); the result names no task PR of another repository",
                    self.api.repo().rsplit_once("/repos/").map_or("the board repository", |x| x.1))),
            },
            r => r?.json()?["status"].as_str().unwrap_or_default().to_string(),
        };
        self.publish_in(&status, None, (n, sha, mode, title, task_line), root, enqueued)
    }

    /// #2047: the `pr` route of PR `p` in repository `repo` (same host and token): its merge state, merge queue and CI,
    /// named in every receipt and reason. An unreadable PR blocks at once; nothing retries it silently.
    fn publish_foreign(&self, repo: &str, pr: Value, task: (u64, &str, &str, &str, &str), root: Option<&std::path::Path>, enqueued: bool) -> Result<Publish> {
        let sha = task.1;
        let p = pr["number"].as_u64().ok_or("publication PR number unknown")?;
        let name = format!("{repo}#{p}");
        let r = self.api.scoped(format!("{}/repos/{repo}", self.api.root), || -> Result<Publish> {
            let read = |url: &str| self.api.call("GET", url, None).and_then(|r| r.json());
            let status = match read(&format!("/compare/main...{sha}")) {
                Err(e) if unreachable(&e) => return Ok(Publish::Blocked(format!("result {sha} unreachable in {repo} of publication PR {name}: {e}"))),
                r => r?["status"].as_str().unwrap_or_default().to_string(),
            };
            self.publish_in(&status, Some(&pr), task, root, enqueued)
        });
        let named = |e: String| if e.contains(&name) { e } else { format!("{name}: {e}") };
        Ok(match r.map_err(named)? {
            Publish::Done(t) => Publish::Done(if t.ends_with("already on main") { format!("{t} of {repo}") } else { t.replacen(&format!("merged #{p}"), &format!("merged #{p} in {repo}"), 1) }),
            Publish::Pending(e) => Publish::Pending(named(e)),
            Publish::Waiting(e) => Publish::Waiting(named(e)),
            Publish::Failed(e) => Publish::Failed(named(e)),
            Publish::MainMoved(e) => Publish::MainMoved(named(e)),
            Publish::Blocked(e) => Publish::Blocked(named(e)),
            Publish::Superseded { reason, merged } => Publish::Superseded { reason: named(reason), merged },
            p => p,
        })
    }

    /// #2047, #2061: the PRs of other repositories on this board's host that the newest `result` (or a `publish` after
    /// it) names in the trusted log, as a link or `owner/name#P`; the last named first, each once.
    fn foreign_prs(&self, n: u64) -> Result<Vec<(String, u64)>> {
        let web = format!("{}/", self.api.root.replace("api.github.com", "github.com").replace("/api/v3", ""));
        let own = self.api.base.rsplit_once("/repos/").map(|x| x.1.to_lowercase()).unwrap_or_default();
        let name_ok = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
        let mut prs: Vec<(String, u64)> = vec![];
        for u in result_links(n, &super::entries(self, n)?).iter().rev() {
            let (repo, number) = match u.strip_prefix(&web) {
                Some(path) => match path.split('/').collect::<Vec<_>>()[..] {
                    [owner, name, "pull", p] => (format!("{owner}/{name}"), p),
                    _ => continue,
                },
                None => match u.split_once('#') { Some((repo, p)) => (repo.to_string(), p), None => continue },
            };
            let Ok(p) = number.parse::<u64>() else { continue };
            let Some((owner, name)) = repo.split_once('/').filter(|(o, r)| name_ok(o) && name_ok(r)) else { continue };
            let repo = format!("{owner}/{name}");
            if repo.to_lowercase() != own && !prs.contains(&(repo.clone(), p)) { prs.push((repo, p)); }
        }
        Ok(prs)
    }

    /// The `pr` and `direct` routes after the ancestry read `status` of the result; `foreign` is the PR of another
    /// repository whose API the caller scoped (#2047), else the board repository's `taskq-<N>` PRs publish.
    fn publish_in(&self, status: &str, foreign: Option<&Value>, (n, sha, mode, title, task_line): (u64, &str, &str, &str, &str), root: Option<&std::path::Path>, enqueued: bool) -> Result<Publish> {
        let branch = format!("taskq-{n}");
        // No commit, or the SHA is already on main: verify ancestry, close.
        if matches!(status, "identical" | "behind") {
            return Ok(Publish::Done(format!("{sha} already on main")));
        }
        if mode == "direct" && foreign.is_some() {
            return Ok(Publish::Blocked("publication PR is in another repository; `direct` moves only the board repository's main".into()));
        }
        if mode == "direct" {
            if let Some(fail) = self.ci(sha)? { return Ok(Publish::Failed(fail)); }
            let head = self.api.call("GET", &format!("/git/ref/heads/{branch}"), None)?.json()?;
            if head["object"]["sha"] != sha {
                return Ok(Publish::Failed(format!("{branch} is not at the result {sha}")));
            }
            self.api.call("PATCH", "/git/refs/heads/main", Some(json!({ "sha": sha, "force": false })))?;
            self.publication_ancestry(sha, mode);
            return Ok(Publish::Done(format!("main fast-forwarded to {sha}")));
        }
        let rules = self.main_rules();
        let manual = rules.as_ref().is_err_and(|e| plan_gap(e));
        let queue = !manual && rules?.as_array().ok_or("unknown main rules")?.iter().any(|r| r["type"] == "merge_queue");
        let owner = self.api.base.rsplit_once("/repos/").map(|x| x.1).and_then(|r| r.split('/').next()).unwrap_or_default().to_string();
        let prs = match foreign {
            Some(pr) if pr["state"] == "open" => vec![pr.clone()],
            Some(_) => vec![],
            None => self.api.call("GET", &format!("/pulls?head={owner}:{branch}&base=main&state=open"), None)?.json()?.as_array().cloned().ok_or("PR list unknown")?,
        };
        if prs.is_empty() {
            // § 7 merged, not closed: a merged PR at the result SHA into any base (a release branch too) is the publication; no second merge
            let closed = match foreign {
                Some(pr) => json!([pr]),
                None => self.api.call("GET", &format!("/pulls?head={owner}:{branch}&state=closed"), None)?.json()?,
            };
            let merged_prs: Vec<&Value> = closed.as_array().into_iter().flatten().filter(|p| !p["merged_at"].is_null()).collect();
            if foreign.is_some() && merged_prs.is_empty() { return Ok(Publish::Blocked(format!("publication PR closed unmerged at result {sha}"))); }
            // #1820 (pilotq #106): a PR merged at a rebased head is the publication when it carries the result's patch
            let found = merged_prs.iter().find(|p| p["head"]["sha"] == sha).map(|p| (*p, String::new()))
                .or_else(|| merged_prs.iter().find_map(|p| self.same_patch(p, sha).ok().flatten()
                    .map(|by| (*p, format!(" (head {:.7}, the result's {by})", s(&p["head"], "sha"))))));
            if let Some((pr, note)) = found {
                let merged = s(pr, "merge_commit_sha");
                let head = s(&pr["head"], "sha");
                let ci = if queue || enqueued {
                    match self.merge_ci(&merged)? {
                        Err(fail) => return Ok(Publish::Blocked(format!("PR #{} merged as {merged}, {fail}", pr["number"]))),
                        Ok(note) => format!("merge-result CI green{note}"),
                    }
                } else if let Some(fail) = self.ci(&head)? { return Ok(Publish::Failed(fail)); } else { format!("exact-SHA CI green on {head}") };
                return Ok(Publish::Done(format!("merged #{}{} as {}{note}; {ci}", pr["number"], into(&s(&pr["base"], "ref")), merged)));
            }
            // #1777: superseded by PRs merged at other heads; `Failed` would cancel a PR that is gone and retry each pass
            let merged: Vec<_> = merged_prs.iter().map(|p| format!("#{} at {:.7}", p["number"], s(&p["head"], "sha"))).collect();
            if merged.is_empty() { return Ok(Publish::Blocked(format!("no open {branch} PR at result {sha}; merged {branch} PRs: none"))); }
            let merged = merged.join(", ");
            return Ok(Publish::Superseded { reason: format!("no open {branch} PR at result {sha}; merged {branch} PRs: {merged}"), merged });
        }
        if prs.len() != 1 {
            return Ok(Publish::Failed(format!("{} open PRs of {branch}, want exactly one", prs.len())));
        }
        let head = prs[0]["head"]["sha"].as_str().ok_or("PR head SHA unknown")?;
        if head != sha { return Ok(head_moved(&prs[0]["number"], head, sha)); }
        // #1935: no queue and no protection on this plan; a person merges, the merged-PR reconcile above closes
        if manual { return Err(format!("PR #{} {}: {PLAN}; merge it: {}", prs[0]["number"], super::MANUAL_MERGE, s(&prs[0], "html_url"))); }
        if queue { return self.queue(&prs[0], sha, enqueued); }
        match self.api.call("GET", "", None)?.json()?["private"].as_bool() {
            Some(true) => {},
            Some(false) => return Err("public board needs a main merge queue with required merge_group CI; Git fallback is private-only".into()),
            None => return Err("repository visibility unknown; no fallback".into()),
        }
        let root = root.ok_or("private fallback requires the service")?;
        if !self.strict_main() { return Err("private fallback needs strict main protection without token bypass".into()); }
        if status == "diverged" {
            if foreign.is_some() { return Ok(Publish::Blocked("private fallback rebases only the board repository's task worktree; rebase the PR in its repository".into())); }
            return self.rebase(root, n, sha);
        }
        if status != "ahead" { return Err("unknown result ancestry; private fallback left unchanged".into()); }
        if let Some(fail) = self.ci(sha)? { return Ok(Publish::Failed(fail)); }
        let p = prs[0]["number"].as_u64().unwrap_or(0);
        if prs[0]["draft"] == true {
            // GitHub refuses to merge a draft (HTTP 405) and REST cannot mark it ready
            let ready = self.api.call("POST", &format!("{}/graphql", self.api.root), Some(json!({
                "query": "mutation($id: ID!) { markPullRequestReadyForReview(input: {pullRequestId: $id}) { pullRequest { isDraft } } }",
                "variables": { "id": prs[0]["node_id"] },
            })))?.json()?;
            if ready["data"]["markPullRequestReadyForReview"]["pullRequest"]["isDraft"] != false {
                return Err(format!("#{p} is still a draft: {}", ready["errors"]));
            }
        }
        let merged = match self.api.call("PUT", &format!("/pulls/{p}/merge"), Some(json!({ "merge_method": "squash", "sha": sha, "commit_title": format!("{title} (#{p})"), "commit_message": task_line }))) {
            Err(e) if e.contains("HTTP 405") || e.contains("HTTP 409") => return Err("main moved or merge unavailable; service will reread the private fallback".into()),
            r => r?,
        };
        let back = self.api.call("GET", &format!("/pulls/{p}"), None)?.json()?;
        if back["merged"] != true {
            if let Ok(second) = self.api.call("GET", &format!("/pulls/{p}"), None).and_then(|r| r.json())
                && second["merged"] != true
            {
                crate::incident::detect("publication.readback", concat!(file!(), ":", line!()), "none", &[mode], &["merge:unconfirmed"]);
            }
        }
        if back["merged"] == true {
            if let Ok(merged) = merged.json() { self.publication_ancestry(&s(&merged, "sha"), mode); }
        }
        if back["merged"] == true { Ok(Publish::Done(format!("merged #{p} as {}; exact-SHA CI green on {sha}", s(&back, "merge_commit_sha")))) } else { Err(format!("#{p} not merged yet")) }
    }
}

impl GitHub {
    /// § 7 (#1820): PR `pr`, merged at another head, carries the result `sha`'s patch. Both diffs start at the merge
    /// commit's first parent, so merge, squash and rebase merges compare alike; any unknown reads as a different patch.
    /// Returns what matched: `patch-id`, or (#2061, dashq#241) `changes` when a rebase moved hunk context or GitHub
    /// omitted a file's patch.
    fn same_patch(&self, pr: &Value, sha: &str) -> Result<Option<&'static str>> {
        let merge = s(pr, "merge_commit_sha");
        if !hex40(&merge) { return Ok(None); }
        let base = self.api.call("GET", &format!("/commits/{merge}"), None)?.json()?["parents"][0]["sha"].as_str().map(String::from).ok_or("merge parent unknown")?;
        let diff = |head: &str| -> Result<Value> { self.api.call("GET", &format!("/compare/{base}...{head}"), None)?.json() };
        let (merged, result) = (diff(&s(&pr["head"], "sha"))?, diff(sha)?);
        Ok(if matches!((patch_id(&merged), patch_id(&result)), (Some(a), Some(b)) if a == b) { Some("patch-id") }
            else if matches!((changes(&merged), changes(&result)), (Some(a), Some(b)) if a == b) { Some("changes") } else { None })
    }
}

/// #2061 (dashq#241): a compare reply's changes per file, without hunk context or positions: its name, prior name and
/// status, then its removed and its added lines, each in order and without whitespace; a file whose patch GitHub
/// omits (binary, too large) is its blob. None when the list may be truncated (300 files).
fn changes(compare: &Value) -> Option<Vec<(String, Vec<String>, Vec<String>)>> {
    let files = compare["files"].as_array().filter(|f| !f.is_empty() && f.len() < 300)?;
    let mut all = vec![];
    for f in files {
        let name = f["filename"].as_str()?;
        let file = format!("{} {} {name}", f["status"].as_str()?, f["previous_filename"].as_str().unwrap_or(name));
        let lines = |sign: char| -> Option<Vec<String>> { match f["patch"].as_str() {
            Some(patch) => Some(patch.lines().filter_map(|l| l.strip_prefix(sign)).map(|l| l.chars().filter(|c| !c.is_whitespace()).collect()).collect()),
            None => Some(vec![format!("blob {}", f["sha"].as_str().filter(|b| hex40(b))?)]),
        } };
        all.push((file, lines('-')?, lines('+')?));
    }
    all.sort();
    Some(all)
}

/// `git patch-id --stable` over a compare reply's per-file diffs: per file, SHA-1 of its header and hunk lines without
/// whitespace or `@@` line numbers; file hashes summed with carry, so file order does not matter.
/// None when GitHub omits a file's patch (binary, too large) or may have truncated the list (300 files).
fn patch_id(compare: &Value) -> Option<[u8; 20]> {
    let files = compare["files"].as_array().filter(|f| !f.is_empty() && f.len() < 300)?;
    let mut sum = [0u8; 20];
    for f in files {
        let name = f["filename"].as_str()?;
        let old = f["previous_filename"].as_str().unwrap_or(name);
        let (from, to) = match f["status"].as_str() { Some("added") => ("/dev/null".into(), format!("b/{name}")),
            Some("removed") => (format!("a/{old}"), "/dev/null".into()), _ => (format!("a/{old}"), format!("b/{name}")) };
        // GitHub sends no file mode: an added or removed file reads as a regular one (100644)
        let mode = match f["status"].as_str() { Some("added") => "new file mode 100644", Some("removed") => "deleted file mode 100644", _ => "" };
        let header = [format!("diff --git a/{old} b/{name}"), mode.into(), format!("--- {from}"), format!("+++ {to}")];
        let mut ctx = ring::digest::Context::new(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY);
        for line in header.iter().map(String::as_str).filter(|l| !l.is_empty()).chain(f["patch"].as_str()?.lines().filter(|l| !l.starts_with("@@"))) {
            ctx.update(line.chars().filter(|c| !c.is_whitespace()).collect::<String>().as_bytes());
        }
        let mut carry = 0u16;
        for (s, d) in sum.iter_mut().zip(ctx.finish().as_ref()) {
            carry += *s as u16 + *d as u16;
            *s = carry as u8;
            carry >>= 8;
        }
    }
    Some(sum)
}

/// #2047: GitHub refuses to show this token the PR or its repository; retrying never changes it.
fn unreachable(e: &str) -> bool {
    ["HTTP 401", "HTTP 403", "HTTP 404"].iter().any(|c| e.contains(c)) && !e.contains("rate limit")
}

/// #2047: the PR links of the newest `result` of task `n` and of the `publish` entries after it, in log order;
/// #2061: `owner/name#P` references too.
fn result_links(n: u64, log: &[crate::core::Entry]) -> Vec<String> {
    let mut links = vec![];
    for e in log.iter().filter(|e| !e.edited) {
        let kind = crate::core::kind(n, &e.body);
        if kind == "result" { links.clear(); }
        if matches!(kind, "result" | "publish") {
            links.extend(e.body.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '<' | '>' | ','))
                .map(|u| u.trim_start_matches(|c: char| !c.is_ascii_alphanumeric()).trim_end_matches(|c: char| !c.is_ascii_alphanumeric()))
                .filter(|u| u.starts_with("https://") && u.contains("/pull/") || !u.contains("://") && u.contains('/') && u.contains('#'))
                .map(String::from));
        }
    }
    links
}

/// #1820 (pilotq #106): a PR pushed after the result (a rebase in review) is unreviewed and never merges; silence held it 50 minutes.
/// `main-moved` requeues the task; the worker reviews the PR head and delivers it as a new result.
fn head_moved(number: &Value, head: &str, sha: &str) -> Publish {
    Publish::MainMoved(format!("PR #{number} head {head:.7} is not the result {sha:.7}; deliver the PR head as a new result"))
}

impl Board for GitHub {
    fn forget(&self) { *self.open.borrow_mut() = None; }
    fn merge_ci_green(&self, sha: &str) -> Result<()> { self.merge_ci(sha)?.map(|_| ()) }
    fn delivery_gap(&self, mode: &str) -> Result<Option<String>> {
        if mode != "pr" { return Ok(None); }
        match self.main_rules() {
            Err(e) if plan_gap(&e) => Ok(Some(format!("{PLAN} (HTTP 403); each PR {}", super::MANUAL_MERGE))),
            r => r.map(|_| None),
        }
    }
    /// The `try_publish` PR route: a merge queue, or a private repository with strict main protection.
    fn pr_route(&self) -> crate::check::Probe {
        use crate::check::Probe;
        let rules = match self.main_rules() {
            Err(e) if plan_gap(&e) => return Probe::Gap(format!("{PLAN} (HTTP 403); each PR {}", super::MANUAL_MERGE)),
            Err(e) => return Probe::Unknown(e),
            Ok(v) => v,
        };
        if rules.as_array().is_some_and(|r| r.iter().any(|r| r["type"] == "merge_queue")) { return Probe::Ok; }
        match self.api.call("GET", "", None).and_then(|r| r.json()).map(|v| v["private"].as_bool()) {
            Ok(Some(false)) => Probe::Gap("main of this public repository has no merge queue".into()),
            Ok(Some(true)) if self.strict_main() => Probe::Ok,
            Ok(Some(true)) => Probe::Gap("this private repository has no merge queue and no strict main protection".into()),
            Ok(None) => Probe::Unknown("repository visibility unknown".into()),
            Err(e) => Probe::Unknown(e),
        }
    }
    fn canonical_labels(&self) -> Result<bool> {
        self.api.canonical_labels()
    }

    fn ensure_labels(&self, names: &[String]) -> Result<()> {
        self.api.ensure_labels(names, |c| c.into(), |current, payload| {
            let (method, path) = match current {
                Some(_) => ("PATCH", format!("/labels/{}", encode(payload["name"].as_str().unwrap_or_default()))),
                None => ("POST", "/labels".into()),
            };
            self.api.call(method, &path, Some(payload)).map(|_| ())
        })
    }

    fn issues(&self, since: Option<i64>) -> Result<Vec<Issue>> {
        let url = match since {
            Some(t) => format!("/issues?state=all&per_page=100&since={}", stamp(t)),
            None if self.api.root == "https://api.github.com" => return self.open_unchanged(),
            None => "/issues?state=open&per_page=100".into(),
        };
        Ok(self.api.all(&url)?.iter().filter(|v| v.get("pull_request").is_none()).map(parse_issue).collect())
    }

    fn closed_tasks(&self, page: u32, since: Option<i64>) -> Result<(Vec<(Issue, i64)>, bool)> {
        let since = since.map_or(String::new(), |t| format!("&since={}", stamp(t)));
        let r = self.api.call("GET", &format!("/issues?state=closed&labels={}&sort=updated&direction=desc&per_page={}&page={}{since}", crate::service::TASK_LABEL, super::CLOSED_PAGE, page.max(1)), None)?;
        let list = r.json()?;
        let rows = list.as_array().ok_or("invalid board list: expected array")?.iter().filter(|v| v.get("pull_request").is_none());
        Ok((rows.map(|v| (parse_issue(v), iso(&s(v, "closed_at")).unwrap_or(0))).collect(), r.next().is_some()))
    }

    fn issue(&self, n: u64) -> Result<Issue> {
        Ok(parse_issue(&self.api.call("GET", &format!("/issues/{n}"), None)?.json()?))
    }

    fn research_tasks(&self) -> Result<Vec<(Issue, i64)>> {
        let mut rows = std::collections::BTreeMap::new();
        for label in ["research", "docs"] {
            for v in self.api.all(&format!("/issues?state=closed&labels=taskq,{label}&per_page=100"))? {
                if v.get("pull_request").is_none() { let i=parse_issue(&v); rows.insert(i.number, (i, iso(&s(&v,"closed_at")).unwrap_or(0))); }
            }
        }
        Ok(rows.into_values().collect())
    }

    fn source(&self, path: &str, sha: &str) -> Result<String> {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let v=self.api.call("GET", &format!("/contents/{}?ref={}",encode(path),encode(sha)),None)?.json()?;
        if v["encoding"]!="base64" {return Err("report source encoding unknown".into());}
        let bytes=STANDARD.decode(s(&v,"content").replace(['\r','\n'],"")).map_err(|_|"report source encoding invalid")?;
        String::from_utf8(bytes).map_err(|_|"report source is not UTF-8".into())
    }

    fn first_admission(&self, n: u64) -> Result<Option<(u64, i64)>> {
        let events = self.api.all(&format!("/issues/{n}/events?per_page=100"))?;
        first_admission(&events)
    }

    fn comments(&self, n: u64) -> Result<Vec<Comment>> {
        Ok(self.api.all(&format!("/issues/{n}/comments?per_page=100"))?.iter().map(parse_comment).collect())
    }

    fn comment(&self, n: u64, body: &str) -> Result<u64> {
        self.api.call("POST", &format!("/issues/{n}/comments"), Some(json!({ "body": body })))?.json()?["id"].as_u64().ok_or("no comment id".into())
    }

    fn edit_comment(&self, _n: u64, id: u64, body: &str) -> Result<()> {
        self.api.call("PATCH", &format!("/issues/comments/{id}"), Some(json!({ "body": body }))).map(|_| ())
    }

    fn labels(&self, n: u64, add: &[String], remove: &[String]) -> Result<()> {
        if !add.is_empty() && self.canonical_labels()? { self.ensure_labels(add)?; }
        if !add.is_empty() {
            self.api.call("POST", &format!("/issues/{n}/labels"), Some(json!({ "labels": add })))?;
        }
        for l in remove {
            // percent-encode: a label like `q-/../..` must not walk the API path
            match self.api.call("DELETE", &format!("/issues/{n}/labels/{}", encode(l)), None) {
                Err(e) if e.contains("HTTP 404") => {}
                r => {
                    r?;
                }
            }
        }
        verify_assignment(self, n, add, remove)
    }

    fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<u64> {
        if let Some(names) = self.api.creation_labels(labels)? { self.ensure_labels(&names)?; }
        let n = self.api.call("POST", "/issues", Some(json!({ "title": title, "body": body, "labels": labels })))?.json()?["number"].as_u64().ok_or("no issue number")?;
        verify_assignment(self, n, labels, &[])?;
        Ok(n)
    }

    fn edit(&self, n: u64, body: &str) -> Result<()> {
        self.api.call("PATCH", &format!("/issues/{n}"), Some(json!({ "body": body }))).map(|_| ())
    }

    fn close(&self, n: u64) -> Result<()> {
        self.api.call("PATCH", &format!("/issues/{n}"), Some(json!({ "state": "closed" }))).map(|_| ())
    }

    fn reopen(&self, n: u64) -> Result<()> {
        self.api.call("PATCH", &format!("/issues/{n}"), Some(json!({ "state": "open" }))).map(|_| ())
    }

    fn now(&self) -> Result<i64> {
        // GitHub does not count `GET /rate_limit` against the budget
        self.api.now(KIND.user_key, Some("/rate_limit"))
    }

    fn me(&self) -> Result<String> {
        self.api.me(KIND.user_key)
    }

    fn defer_publication(&self, mode: &str) -> bool { mode == "pr" }

    fn cancel_publication(&self, n: u64, sha: &str) -> Result<()> {
        let owner = self.api.base.rsplit_once("/repos/").and_then(|r| r.1.split('/').next()).ok_or("repository owner unknown")?;
        let prs = self.api.call("GET", &format!("/pulls?head={owner}:taskq-{n}&base=main&state=open"), None)?.json()?;
        let prs = prs.as_array().ok_or("cancellation PR list unknown")?;
        // #2047: no task PR here; the result's PR in another repository is the publication to cancel
        if prs.is_empty() && let Some((repo, p)) = self.foreign_prs(n)?.into_iter().next() {
            let name = format!("{repo}#{p}");
            let pr = self.api.call("GET", &format!("{}/repos/{repo}/pulls/{p}", self.api.root), None)
                .and_then(|r| r.json()).map_err(|e| format!("publication PR {name} unreachable; verify merge before cancelling: {e}"))?;
            if pr["head"]["sha"].as_str().ok_or_else(|| format!("{name} cancellation head unknown"))? != sha { return Ok(()); }
            return self.dequeue(pr["node_id"].as_str().ok_or_else(|| format!("{name} cancellation PR id unknown"))?).map_err(|e| format!("{name}: {e}"));
        }
        if prs.len() != 1 { return Err("publication PR absent or ambiguous; verify merge before cancelling".into()); }
        if prs[0]["head"]["sha"].as_str().ok_or("cancellation head unknown")? != sha { return Ok(()); }
        self.dequeue(prs[0]["node_id"].as_str().ok_or("cancellation PR id unknown")?)
    }

    fn close_task_prs(&self, n: u64, reason: &str) -> Result<Vec<u64>> {
        let owner = self.api.base.rsplit_once("/repos/").and_then(|r| r.1.split('/').next()).ok_or("repository owner unknown")?;
        let prs = self.api.call("GET", &format!("/pulls?head={owner}:taskq-{n}&state=open"), None)?.json()?;
        let marker = format!("Task #{n} closed without merging this PR");
        let mut closed = vec![];
        for pr in prs.as_array().ok_or("task PR list unknown")? {
            let p = pr["number"].as_u64().ok_or("task PR number unknown")?;
            self.dequeue(pr["node_id"].as_str().ok_or("task PR id unknown")?)?;
            // ponytail: two hosts racing between this read and the post both comment; a guard comment is the upgrade path
            if !self.comments(p)?.iter().any(|c| c.body.starts_with(&marker)) {
                self.comment(p, &format!("{marker}: {reason}\nBranch `taskq-{n}` kept. Task: {}", self.url(n)))?;
            }
            let back = self.api.call("PATCH", &format!("/pulls/{p}"), Some(json!({ "state": "closed" })))?.json()?;
            if back["state"] != "closed" || back["merged"] == true { return Err(format!("PR #{p} close not read back")); }
            closed.push(p);
        }
        Ok(closed)
    }

    // § 7 Result and close verdicts: a pending publication takes the current preset (#1942: was always `pr`)
    fn resume_publication(&self, n: u64, sha: &str, mode: &str, title: &str, task_line: &str, root: &std::path::Path, enqueued: bool) -> Publish {
        outcome(self.try_publish(n, sha, mode, title, task_line, Some(root), enqueued))
    }

    fn publish(&self, n: u64, sha: &str, mode: &str, title: &str, task_line: &str) -> Publish {
        outcome(self.try_publish(n, sha, mode, title, task_line, None, false))
    }

    fn requests(&self) -> crate::requests::Budget {
        self.api.credentials.borrow().budget.borrow().clone()
    }

    fn url(&self, n: u64) -> String {
        self.foreign_url(self.api.base.rsplit_once("/repos/").map(|x| x.1).unwrap_or_default(), n)
    }

    fn foreign(&self, repo: &str, n: u64) -> Result<Issue> {
        let v = self.api.call("GET", &format!("{}/repos/{}/issues/{n}", self.api.root, self.api.sibling("/repos/", repo)), None)?.json()?;
        Ok(parse_issue(&super::dep_reply(v, "number", n, "open")?))
    }

    fn foreign_url(&self, repo: &str, n: u64) -> String {
        let web = self.api.root.replace("api.github.com", "github.com").replace("/api/v3", "");
        format!("{web}/{}/issues/{n}", self.api.sibling("/repos/", repo))
    }

    /// One commit on the orphan `taskq-evidence` branch per image; the URL pins that commit, so it never moves.
    fn upload_file(&self, n: u64, path: &std::path::Path) -> Result<String> {
        let (sha, file, bytes) = self.evidence_commit(n, path, &format!("heads/{EVIDENCE}"))?;
        let public = super::video_kind(&bytes).is_some() && self.api.root == "https://api.github.com"
            && self.api.call("GET", "", None)?.json()?["private"] == false;
        Ok(if public { cdn_url(&self.api.base, &sha, &file) } else { raw_url(&self.api.root, &self.api.base, &sha, &file) })
    }

    /// § 2 evidence files (#2019): one commit per file under the non-branch ref `refs/taskq/evidence`, so a default
    /// clone never downloads it (#2016); the commit-pinned raw URL downloads the file in one click.
    fn upload_attachment(&self, n: u64, path: &std::path::Path) -> Result<String> {
        let (sha, file, _) = self.evidence_commit(n, path, ATTACHMENTS)?;
        Ok(raw_url(&self.api.root, &self.api.base, &sha, &file))
    }

    /// GitHub raw serves `.html` and `.htm` as `text/plain`: the browser shows the page source instead of saving it.
    fn html_downloads(&self) -> bool { false }

    fn file(&self, url: &str) -> Result<Vec<u8>> {
        use base64::Engine;
        let (sha, file) = raw_path(&self.api.root, &self.api.base, url).ok_or_else(|| format!("image unknown: {url} is not a taskq evidence image of this repository"))?;
        // the contents API carries files up to 1 MB inline; the blob API reads larger ones by sha
        let meta = self.api.call("GET", &format!("/contents/{}?ref={sha}", file.split('/').map(encode).collect::<Vec<_>>().join("/")), None)?.json()?;
        let blob = self.api.call("GET", &format!("/git/blobs/{}", s(&meta, "sha")), None)?.json()?;
        base64::engine::general_purpose::STANDARD.decode(s(&blob, "content").replace('\n', "")).map_err(|e| format!("image {url}: {e}"))
    }
}

impl GitHub {
    /// Commits the file `path` as `<n>/<digest>-<name>` on top of ref `git_ref` (`heads/<branch>` or a non-branch ref
    /// below `refs/`), creating the ref as an orphan when missing; returns the commit, the file path and its bytes.
    /// ponytail: a concurrent upload moving the ref fails the PATCH (not a fast-forward); the caller retries the command.
    fn evidence_commit(&self, n: u64, path: &std::path::Path, git_ref: &str) -> Result<(String, String, Vec<u8>)> {
        use base64::Engine;
        let bytes = std::fs::read(path).map_err(|e| format!("file {}: {e}", path.display()))?;
        let digest: String = ring::digest::digest(&ring::digest::SHA256, &bytes).as_ref()[..6].iter().map(|b| format!("{b:02x}")).collect();
        let file = format!("{n}/{digest}-{}", image_name(path));
        let parent = match self.api.call("GET", &format!("/git/ref/{git_ref}"), None) {
            Ok(r) => Some(s(&r.json()?["object"], "sha")),
            Err(e) if e.contains("HTTP 404") => None,
            Err(e) => return Err(e),
        };
        let blob = self.api.call("POST", "/git/blobs", Some(json!({"content": base64::engine::general_purpose::STANDARD.encode(&bytes), "encoding": "base64"})))?.json()?;
        let mut tree = json!({"tree": [{"path": file, "mode": "100644", "type": "blob", "sha": blob["sha"]}]});
        if let Some(p) = &parent {
            tree["base_tree"] = self.api.call("GET", &format!("/git/commits/{p}"), None)?.json()?["tree"]["sha"].clone();
        }
        let tree = self.api.call("POST", "/git/trees", Some(tree))?.json()?;
        let commit = self.api.call("POST", "/git/commits", Some(json!({"message": format!("taskq evidence #{n}"), "tree": tree["sha"], "parents": parent.iter().collect::<Vec<_>>()})))?.json()?;
        let sha = s(&commit, "sha");
        match parent {
            Some(_) => self.api.call("PATCH", &format!("/git/refs/{git_ref}"), Some(json!({"sha": sha})))?,
            None => self.api.call("POST", "/git/refs", Some(json!({"ref": format!("refs/{git_ref}"), "sha": sha})))?,
        };
        Ok((sha, file, bytes))
    }
}

/// The branch that holds § 2 visual evidence images: an orphan, so no code history reaches it.
const EVIDENCE: &str = "taskq-evidence";

/// § 2 evidence files (#2019): a ref outside `refs/heads`, which default clones do not fetch (#2016).
const ATTACHMENTS: &str = "taskq/evidence";

fn web(root: &str, base: &str) -> String {
    format!("{}/{}", root.replace("api.github.com", "github.com").replace("/api/v3", ""), base.rsplit_once("/repos/").map(|x| x.1).unwrap_or_default())
}

/// § 2 visual evidence (#1782): the absolute, commit-pinned URL of an evidence image; GitHub renders it for every
/// reader of the repository.
fn raw_url(root: &str, base: &str, sha: &str, file: &str) -> String {
    format!("{}/raw/{sha}/{file}", web(root, base))
}

/// § 2 visual evidence videos (#1844): GitHub serves a raw MP4 or WebM as a download, so a public github.com repository
/// links the same commit-pinned file through jsDelivr, which serves it with its video type and the browser plays it.
fn cdn_url(base: &str, sha: &str, file: &str) -> String {
    format!("https://cdn.jsdelivr.net/gh/{}@{sha}/{file}", base.rsplit_once("/repos/").map(|x| x.1).unwrap_or_default())
}

/// The commit and file of an evidence URL of this repository, raw or jsDelivr; none for any other URL.
fn raw_path<'a>(root: &str, base: &str, url: &'a str) -> Option<(&'a str, &'a str)> {
    let cdn = (root == "https://api.github.com").then(|| cdn_url(base, "", "")).and_then(|p| url.strip_prefix(p.trim_end_matches('/')));
    let (sha, file) = cdn.or_else(|| url.strip_prefix(&format!("{}/raw/", web(root, base))))?.split_once('/')?;
    (hex40(sha) && !file.is_empty() && !file.split('/').any(|p| p.is_empty() || p == "..")).then_some((sha, file))
}

/// Complete paginated board history, never issue creation or current queue size.
/// A deleted label's event has no label (§ 4 admission records): it admits nothing.
fn first_admission(events: &[Value]) -> Result<Option<(u64, i64)>> {
    events.iter().filter(|v| v["event"] == "labeled" && v["label"]["name"] == "taskq")
        .map(|v| Ok((v["id"].as_u64().filter(|id| *id > 0).ok_or("admission ID unavailable")?,
            iso(&s(v, "created_at")).filter(|at| *at > 0).ok_or("admission time unavailable")?)))
        .collect::<Result<Vec<_>>>().map(|events| events.into_iter().min_by_key(|(id, at)| (*at, *id)))
}

#[cfg(test)]
mod admission_tests {
    use super::*;
    #[test]
    fn earliest_label_event_is_the_only_arrival() {
        let mut events = json!([{"id":9,"event":"labeled","action":"add","label":{"name":"taskq"},"created_at":"2026-10-30T12:01:00Z"},{"id":3,"event":"labeled","action":"add","label":{"name":"taskq"},"created_at":"2026-10-30T12:00:00Z"},{"id":2,"event":"unlabeled","action":"remove","label":{"name":"taskq"},"created_at":"2026-10-30T11:59:00Z"}]);
        assert_eq!(first_admission(events.as_array().unwrap()).unwrap(), Some((3, iso("2026-10-30T12:00:00Z").unwrap())));
        events[1]["created_at"] = Value::Null;
        assert!(first_admission(events.as_array().unwrap()).is_err());
        assert_eq!(first_admission(&[]).unwrap(), None);
    }

    // #1960 defect: a deleted label's events (label null) failed the source on every pass.
    // Chain: §4 admission records. Risk kept: a deleted label invents or hides a `taskq` admission.
    #[test]
    fn deleted_label_events_admit_nothing() {
        let deleted = json!([{"id":5,"event":"labeled","label":null,"created_at":"2026-10-05T10:00:00Z"},{"id":6,"event":"unlabeled","label":null,"created_at":"2026-10-05T11:00:00Z"}]);
        assert_eq!(first_admission(deleted.as_array().unwrap()).unwrap(), None);
        let mut events = deleted.as_array().unwrap().clone();
        events.push(json!({"id":7,"event":"labeled","label":{"name":"taskq"},"created_at":"2026-10-06T10:00:00Z"}));
        assert_eq!(first_admission(&events).unwrap(), Some((7, iso("2026-10-06T10:00:00Z").unwrap())));
    }
}

#[cfg(test)]
mod token_tests {
    use super::*;

    // #1840 A2: a known repository-access refusal has a plain cause; other failures retain their diagnostic.
    #[test]
    fn repository_access_failure_is_plain() {
        for (kind, expected) in [("NOT_FOUND", "no access to the repository"), ("FORBIDDEN", "GraphQL open issues:")] {
            let mut api = api("github.com", "o/r", "invented-token");
            api.transport = Some(Box::new(move |_, _, _| Ok(super::super::Reply {status:200, headers:vec![], operation:"read",
                body:json!({"errors":[{"type":kind,"message":"invented refusal"}]}).to_string()})));
            let board = GitHub {api, rules:Default::default(), open:Default::default()};
            assert!(board.open_issues().unwrap_err().starts_with(expected));
        }
    }

    // #1840 A3 / section 9: one opened board uses the next credential on the wire, with no restart or login fallback.
    #[test]
    fn next_read_uses_replaced_host_token() {
        use std::{io::{Read, Write}, net::TcpListener, time::{Duration, Instant}};
        let dir = std::env::temp_dir().join(format!("taskq-rotate-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("token"), "invented-first-token").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let root = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for name in ["first", "second"] {
                let until = Instant::now() + Duration::from_secs(60);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < until, "board read never reached the local fixture");
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(e) => panic!("fixture accept: {e}"),
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
                let mut bytes = vec![];
                while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    let mut part = [0; 4096];
                    let n = socket.read(&mut part).unwrap();
                    assert!(n > 0 && bytes.len() + n <= 65536, "invalid fixture request");
                    bytes.extend_from_slice(&part[..n]);
                }
                assert!(String::from_utf8_lossy(&bytes).to_ascii_lowercase().contains(&format!("authorization: bearer invented-{name}-token")));
                let body = json!({"number":1,"title":name,"state":"open","author_association":"OWNER"}).to_string();
                write!(socket,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        // Keep the production credential lookup, Kind::open and GitHub adapter; substitute only the HTTP endpoint.
        let kind = super::super::Kind { api: |root, repo, token| Api::new(root.into(),format!("{root}/repos/{repo}"),token), ..KIND };
        let board = kind.open(&root, "o/r", None, &crate::platform::tool(&dir)).unwrap();
        std::fs::write(crate::requests::user_file(&dir, &root, "invented-first-token"), "kept-login").unwrap();
        assert_eq!(board.me().as_deref(), Ok("kept-login"), "the kept login answers without GET /user");
        let first = board.issue(1).map(|issue| issue.title);
        let old_pause = crate::requests::pause_file(&dir, &root, "invented-first-token");
        std::fs::write(&old_pause, (super::super::wall() + 60).to_string()).unwrap();
        std::fs::write(dir.join("replacement"), "invented-second-token").unwrap();
        std::fs::remove_file(dir.join("token")).unwrap();
        std::fs::rename(dir.join("replacement"), dir.join("token")).unwrap();
        let second = board.issue(1).map(|issue| issue.title);
        let joined = server.join();
        std::fs::remove_file(dir.join("token")).unwrap();
        let missing = board.issue(1).err();
        assert!(old_pause.exists(), "rotation preserves the old credential's pause");
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!(first.as_deref(), Ok("first"));
        assert_eq!(second.as_deref(), Ok("second"));
        assert!(joined.is_ok(), "replacement credential was not observed on the wire");
        assert_eq!(missing.as_deref(), Some(TOKEN_ERROR));
    }

    // Chain: host credential -> service board and worker environment. Risk: login fallback or mismatched errors.
    #[test]
    fn service_and_workers_share_the_host_credential_and_missing_token_error() {
        let dir = std::env::temp_dir().join(format!("taskq-token-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        let tool = crate::platform::tool(&dir);
        for contents in [None, Some(" \n")] {
            if let Some(contents) = contents { std::fs::write(dir.join("token"), contents).unwrap(); }
            let service = KIND.open("github.com", "o/r", None, &tool).err().unwrap();
            assert_eq!(service, TOKEN_ERROR);
            assert_eq!(KIND.worker_env(&tool, "github.com").unwrap_err(), service);
        }
        std::fs::write(dir.join("token"), "existing-owner-token\n").unwrap();
        let env = KIND.worker_env(&tool, "github.com").unwrap();
        assert_eq!(env, vec![("GH_TOKEN".into(), "existing-owner-token".into()), ("GITHUB_TOKEN".into(), "existing-owner-token".into())]);
        assert!(KIND.open("github.com", "o/r", None, &tool).is_ok());
        std::fs::remove_file(dir.join("token")).unwrap();
        std::fs::create_dir(dir.join("token")).unwrap();
        assert_eq!(KIND.worker_env(&tool, "github.com").unwrap_err(), TOKEN_ERROR);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;

    // Chain: §7 Result and close verdicts: the patch-id of a merged head (#1820) is Git's own `git patch-id --stable`.
    // Risk: a home-made hash that drops what Git keeps (context, file names, modes) closes a task on another patch.
    #[test]
    fn patch_id_is_gits_stable_patch_id() {
        let hex = |id: [u8; 20]| id.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let modified = json!({"filename": "src/a.rs", "status": "modified", "patch": "@@ -1,2 +1,2 @@
 fn a() {
-old();
+new();"});
        let added = json!({"filename": "b.txt", "status": "added", "patch": "@@ -0,0 +1 @@
+hi"});
        // expected: `git patch-id --stable` over the same diffs as `git diff` prints them
        assert_eq!(patch_id(&json!({"files": [modified]})).map(hex).as_deref(), Some("ee8eda765c6017d8c75984d367ee840dff07151a"));
        assert_eq!(patch_id(&json!({"files": [added, modified]})).map(hex).as_deref(), Some("91a982717c3ffcb3a0c0b1404c10f5ecfada4861"));
        assert_eq!(patch_id(&json!({"files": [{"filename": "x.png", "status": "added"}]})), None, "an omitted patch is unknown");
        assert_eq!(patch_id(&json!({"files": []})), None);
    }

    // Chain: § 2 visual evidence on GitHub (#1782). Risk: the load check or the app copy reads a URL outside this
    // repository's commit-pinned evidence, or a relative link passes as an image.
    #[test]
    fn github_evidence_urls_are_absolute_and_commit_pinned() {
        let (root, base) = ("https://api.github.com", "https://api.github.com/repos/o/r");
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let url = raw_url(root, base, sha, "1782/abc-front.png");
        assert_eq!(url, format!("https://github.com/o/r/raw/{sha}/1782/abc-front.png"));
        assert_eq!(raw_path(root, base, &url), Some((sha, "1782/abc-front.png")));
        assert_eq!(raw_path(root, base, "https://github.com/o/r/raw/taskq-evidence/1782/a.png"), None, "branch, not commit");
        assert_eq!(raw_path(root, base, &format!("https://github.com/o/x/raw/{sha}/a.png")), None, "other repository");
        assert_eq!(raw_path(root, base, &format!("https://github.com/o/r/raw/{sha}/../a.png")), None);
        assert_eq!(raw_path(root, base, "/uploads/x/a.png"), None, "relative link");
        let ghe = ("https://ghe.example/api/v3", "https://ghe.example/api/v3/repos/o/r");
        assert_eq!(raw_url(ghe.0, ghe.1, sha, "1/a.png"), format!("https://ghe.example/o/r/raw/{sha}/1/a.png"));
    }

    // Chain: § 2 visual evidence videos on GitHub (#1844). Defect: GitHub raw serves an MP4 as a download, so csgo#415's
    // videos never played from the task. Risk: a jsDelivr URL of another repository, a branch or Enterprise is load-checked.
    #[test]
    fn github_public_videos_link_the_commit_pinned_file_through_jsdelivr() {
        let (root, base) = ("https://api.github.com", "https://api.github.com/repos/o/r");
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let url = cdn_url(base, sha, "1844/abc-run.mp4");
        assert_eq!(url, format!("https://cdn.jsdelivr.net/gh/o/r@{sha}/1844/abc-run.mp4"));
        assert_eq!(raw_path(root, base, &url), Some((sha, "1844/abc-run.mp4")));
        assert_eq!(raw_path(root, base, "https://cdn.jsdelivr.net/gh/o/r@taskq-evidence/1844/a.mp4"), None, "branch, not commit");
        assert_eq!(raw_path(root, base, &format!("https://cdn.jsdelivr.net/gh/o/rx@{sha}/a.mp4")), None, "other repository");
        let ghe = ("https://ghe.example/api/v3", "https://ghe.example/api/v3/repos/o/r");
        assert_eq!(raw_path(ghe.0, ghe.1, &cdn_url(ghe.1, sha, "1/a.mp4")), None, "jsDelivr cannot reach Enterprise");
    }
}
