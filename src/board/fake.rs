//! A board in memory and a replying HTTP adapter, for tests only.

use super::*;

/// A `kind` adapter whose HTTP requests `reply` answers (method, URL -> JSON body or error), for tests outside this module.
pub fn replying(
    kind: &Kind,
    reply: impl Fn(&str, &str) -> Result<Value> + 'static,
) -> Box<dyn Board> {
    replying_to(kind, move |method, url, _| reply(method, url))
}

/// [`replying`] that also sees each request body (GraphQL queries).
pub fn replying_to(
    kind: &Kind,
    reply: impl Fn(&str, &str, Option<Value>) -> Result<Value> + 'static,
) -> Box<dyn Board> {
    let mut api = (kind.api)(
        if kind.name == "github" {
            "github.com"
        } else {
            "gitlab.example"
        },
        "o/r",
        "test",
    );
    api.transport = Some(Box::new(move |method, url, body| {
        reply(method, url, body).map(|v| Reply {
            status: 200,
            headers: vec![],
            body: v.to_string(),
            operation: operation(method),
        })
    }));
    (kind.board)(api)
}

/// A board in memory for tests: one clock tick per write, comment ids shared across issues like real boards.
#[derive(Default)]
pub struct Fake {
    pub state: RefCell<FakeState>,
}

#[derive(Default)]
pub struct FakeState {
    pub issues: Vec<Issue>,
    pub comments: Vec<(u64, Comment)>,
    pub admissions: std::collections::BTreeMap<u64, Result<Option<(u64, i64)>>>,
    pub clock: i64,
    pub next_id: u64,
    /// `pending` or `failed` makes `publish` refuse; anything else merges.
    pub publish: String,
    pub evidence: Vec<crate::readiness::Publication>,
    pub hide_latest_comment: bool,
    /// Writers keep the § 2 summary comment on this board.
    pub summaries: bool,
    pub cancelled_publications: Vec<(u64, String)>,
    pub fail_publication_cancel: bool,
    /// `close_task_prs` calls `(task, reason)`; `fail_task_prs` makes them unknown.
    pub closed_task_prs: Vec<(u64, String)>,
    pub fail_task_prs: bool,
    pub fail_issue_read: Option<(u64, usize)>,
    pub fail_labels: bool,
    /// Pause a label writer while another service reconciles the same task.
    pub before_labels: Option<Box<dyn FnOnce(&Fake, u64)>>,
    /// The next this many `comment` calls fail like a board rate-limit pause.
    pub fail_comments: usize,
    /// The write commits, but its response is lost.
    pub lose_comment_response: bool,
    /// The next this many `comments` reads fail.
    pub fail_comment_reads: usize,
    pub fail_comment_read: Option<(u64, usize)>,
    /// A concurrent writer: this body lands just before the next `comment`.
    pub race: Option<String>,
    /// Concurrent comments land after this task's numbered comment-list read returned its snapshot.
    pub after_comment_read: Option<(u64, usize, Vec<String>)>,
    /// Issues of other repositories, `(repo, issue)`.
    pub foreign: Vec<(String, Issue)>,
    /// Issue reads, `foreign` included, for budget checks.
    pub reads: usize,
    /// The next open-list read returns this older snapshot, like a reused or lagging board list (#1873).
    pub stale_list: Option<Vec<Issue>>,
    /// Open-list reads, visible outside the thread that owns this board.
    pub lists: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    /// Comment-list reads and publication attempts, for request-count checks (#1359).
    pub comment_reads: usize,
    pub publications: usize,
    /// Uploaded images by URL; `bad_image` makes the board serve other bytes back.
    pub images: Vec<(String, Vec<u8>)>,
    pub bad_image: bool,
    /// The board plays an uploaded video inline (GitLab), else it links one (GitHub).
    pub video_inline: bool,
    /// Immutable repository text for the Reports source adapter (path, SHA, body).
    pub report_sources: Vec<(String, String, String)>,
    /// Every report source read fails with this board error (#2193).
    pub source_error: Option<Error>,
    pub report_closed: std::collections::BTreeMap<u64, i64>,
    /// A real adapter that answers `publish` calls instead of the `publish` string above (#1939).
    pub publisher: Option<Box<dyn Board>>,
    pub closed_page_advance: i64,
}

impl FakeState {
    fn tick(&mut self) -> i64 {
        self.clock += 1;
        self.clock
    }
    fn get(&mut self, n: u64) -> Result<&mut Issue> {
        self.issues
            .iter_mut()
            .find(|i| i.number == n)
            .ok_or_else(|| format!("no issue #{n}").into())
    }
}

impl Fake {
    pub fn advance(&self, secs: i64) {
        self.state.borrow_mut().clock += secs;
    }
    /// A comment by an outsider or an edited one, for trust tests.
    pub fn raw(&self, n: u64, body: &str, trusted: bool, edited: bool) -> u64 {
        let mut st = self.state.borrow_mut();
        st.next_id += 1;
        let id = st.next_id;
        let t = st.tick();
        if let Ok(i) = st.get(n) {
            i.updated = t;
        }
        st.comments.push((
            n,
            Comment {
                id,
                created: t,
                body: body.into(),
                trusted,
                edited,
                author: String::new(),
                read: Some(false),
            },
        ));
        id
    }
}

impl Fake {
    /// Closed research and docs tasks with their close times: the fixture Reports rows come from.
    pub fn research_tasks(&self) -> Result<Vec<(Issue, i64)>> {
        let st = self.state.borrow();
        Ok(st
            .issues
            .iter()
            .filter(|i| !i.open && i.labels.iter().any(|l| l == "research" || l == "docs"))
            .map(|i| {
                (
                    i.clone(),
                    st.report_closed
                        .get(&i.number)
                        .copied()
                        .unwrap_or(i.updated),
                )
            })
            .collect())
    }
}

impl Board for Fake {
    fn first_admission(&self, n: u64) -> Result<Option<(u64, i64)>> {
        self.state
            .borrow()
            .admissions
            .get(&n)
            .cloned()
            .unwrap_or(Ok(None))
    }
    fn closed_labelled(&self, label: &str) -> Result<Vec<(Issue, i64)>> {
        let st = self.state.borrow();
        Ok(st
            .issues
            .iter()
            .filter(|i| !i.open && i.labels.iter().any(|l| l == label))
            .map(|i| {
                (
                    i.clone(),
                    st.report_closed
                        .get(&i.number)
                        .copied()
                        .or(i.closed)
                        .unwrap_or(i.updated),
                )
            })
            .collect())
    }
    fn source(&self, path: &str, sha: &str) -> Result<String> {
        if let Some(e) = &self.state.borrow().source_error {
            return Err(e.clone());
        }
        self.state
            .borrow()
            .report_sources
            .iter()
            .find(|(p, s, _)| p == path && s == sha)
            .map(|(_, _, b)| b.clone())
            .ok_or(Error::http(404, "report source unavailable: HTTP 404"))
    }
    fn upload_file(&self, n: u64, path: &std::path::Path) -> Result<String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let mut st = self.state.borrow_mut();
        let url = format!(
            "https://board.example/{n}/{}-{}",
            st.images.len(),
            image_name(path)
        );
        st.images.push((url.clone(), bytes));
        Ok(url)
    }
    fn file(&self, url: &str) -> Result<Vec<u8>> {
        let st = self.state.borrow();
        let b = st
            .images
            .iter()
            .find(|i| i.0 == url)
            .map(|i| i.1.clone())
            .ok_or(format!("image unknown: {url}"))?;
        Ok(if st.bad_image {
            b[..b.len() / 2].to_vec()
        } else {
            b
        })
    }
    fn video_inline(&self) -> bool {
        self.state.borrow().video_inline
    }
    fn closed_tasks(&self, page: u32, since: Option<i64>) -> Result<(Vec<(Issue, i64)>, bool)> {
        let delay = self.state.borrow().closed_page_advance;
        self.advance(delay);
        let mut all: Vec<Issue> = self
            .state
            .borrow()
            .issues
            .iter()
            .filter(|i| !i.open && since.is_none_or(|t| i.updated >= t))
            .cloned()
            .collect();
        all.sort_by_key(|i| std::cmp::Reverse(i.updated));
        let start = (page.max(1) as usize - 1) * CLOSED_PAGE;
        Ok((
            all.iter()
                .skip(start)
                .take(CLOSED_PAGE)
                .map(|i| {
                    (
                        i.clone(),
                        self.state
                            .borrow()
                            .report_closed
                            .get(&i.number)
                            .copied()
                            .unwrap_or(i.updated),
                    )
                })
                .collect(),
            all.len() > start + CLOSED_PAGE,
        ))
    }
    fn issues(&self, since: Option<i64>) -> Result<Vec<Issue>> {
        if let Some(lists) = &self.state.borrow().lists {
            lists.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        if let Some(list) = self.state.borrow_mut().stale_list.take() {
            return Ok(list);
        }
        Ok(self
            .state
            .borrow()
            .issues
            .iter()
            .filter(|i| since.map_or(i.open, |t| i.updated >= t))
            .cloned()
            .collect())
    }
    fn issue(&self, n: u64) -> Result<Issue> {
        let mut st = self.state.borrow_mut();
        if let Some((task, remaining)) = &mut st.fail_issue_read {
            if *task == n {
                *remaining -= 1;
                if *remaining == 0 {
                    st.fail_issue_read = None;
                    return Err("issue read failed".into());
                }
            }
        }
        st.reads += 1;
        drop(st);
        self.state.borrow_mut().get(n).cloned()
    }
    fn foreign(&self, repo: &str, n: u64) -> Result<Issue> {
        let mut st = self.state.borrow_mut();
        st.reads += 1;
        st.foreign
            .iter()
            .find(|(r, i)| r == repo && i.number == n)
            .map(|x| x.1.clone())
            .ok_or(Error::http(404, format!("HTTP 404 {repo}#{n}")))
    }
    fn foreign_url(&self, repo: &str, n: u64) -> String {
        format!("https://board/{repo}/{n}")
    }
    fn comments(&self, n: u64) -> Result<Vec<Comment>> {
        {
            let mut st = self.state.borrow_mut();
            if let Some((task, remaining)) = &mut st.fail_comment_read {
                if *task == n {
                    *remaining -= 1;
                    if *remaining == 0 {
                        st.fail_comment_read = None;
                        return Err("comment read failed".into());
                    }
                }
            }
        }
        if self.state.borrow().fail_comment_reads > 0 {
            self.state.borrow_mut().fail_comment_reads -= 1;
            return Err("comment read failed".into());
        }
        self.state.borrow_mut().comment_reads += 1;
        let st = self.state.borrow();
        let out = st
            .comments
            .iter()
            .filter(|(i, c)| *i == n && !(st.hide_latest_comment && c.id == st.next_id))
            .map(|(_, c)| c.clone())
            .collect();
        let race = st
            .after_comment_read
            .as_ref()
            .is_some_and(|(task, read, _)| *task == n && *read == st.comment_reads);
        drop(st);
        if race {
            let (_, _, bodies) = self.state.borrow_mut().after_comment_read.take().unwrap();
            for body in bodies {
                self.raw(n, &body, true, false);
            }
        }
        Ok(out)
    }
    fn comment(&self, n: u64, body: &str) -> Result<u64> {
        self.state.borrow_mut().get(n)?;
        if self.state.borrow().fail_comments > 0 {
            self.state.borrow_mut().fail_comments -= 1;
            return Err("rate limit: board requests paused".into());
        }
        let race = self.state.borrow_mut().race.take();
        if let Some(other) = race {
            self.raw(n, &other, true, false);
        }
        let id = self.raw(n, body, true, false);
        if std::mem::take(&mut self.state.borrow_mut().lose_comment_response) {
            return Err("comment response lost".into());
        }
        Ok(id)
    }
    fn edit_comment(&self, n: u64, id: u64, body: &str) -> Result<()> {
        let mut st = self.state.borrow_mut();
        let c = st
            .comments
            .iter_mut()
            .find(|(i, c)| *i == n && c.id == id)
            .ok_or(format!("no comment {id}"))?;
        if c.1.body != body {
            c.1.body = body.into();
            c.1.edited = true;
        }
        Ok(())
    }
    fn react_read(&self, n: u64, id: u64) -> Result<()> {
        let mut st = self.state.borrow_mut();
        st.comments
            .iter_mut()
            .find(|(i, c)| *i == n && c.id == id)
            .ok_or(format!("no comment {id}"))?
            .1
            .read = Some(true);
        Ok(())
    }
    fn summaries(&self) -> bool {
        self.state.borrow().summaries
    }
    fn labels(&self, n: u64, add: &[String], remove: &[String]) -> Result<()> {
        let paused = self.state.borrow_mut().before_labels.take();
        if let Some(run) = paused {
            run(self, n);
        }
        let mut st = self.state.borrow_mut();
        if std::mem::take(&mut st.fail_labels) {
            return Err("label write failed".into());
        }
        let t = st.tick();
        let i = st.get(n)?;
        i.labels.retain(|l| !remove.contains(l));
        for l in add {
            if !i.labels.contains(l) {
                i.labels.push(l.clone());
            }
        }
        i.updated = t;
        Ok(())
    }
    fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<u64> {
        let mut st = self.state.borrow_mut();
        let t = st.tick();
        st.next_id += 1;
        let number = st.issues.len() as u64 + 1;
        st.issues.push(Issue {
            number,
            title: title.into(),
            body: body.into(),
            labels: labels.to_vec(),
            assignees: vec![],
            updated: t,
            created: t,
            closed: None,
            open: true,
            trusted: true,
            reason: None,
        });
        Ok(number)
    }
    fn edit(&self, n: u64, body: &str) -> Result<()> {
        let mut st = self.state.borrow_mut();
        let t = st.tick();
        let i = st.get(n)?;
        // like GitHub: an unchanged body keeps `updated_at`
        if i.body != body {
            i.body = body.into();
            i.updated = t;
        }
        Ok(())
    }
    fn close(&self, n: u64) -> Result<()> {
        let mut st = self.state.borrow_mut();
        let t = st.tick();
        let i = st.get(n)?;
        i.open = false;
        i.closed = Some(t);
        i.reason = Some("completed".into());
        i.updated = t;
        Ok(())
    }
    fn reopen(&self, n: u64) -> Result<()> {
        let mut st = self.state.borrow_mut();
        let t = st.tick();
        let i = st.get(n)?;
        i.open = true;
        i.closed = None;
        i.reason = None;
        i.updated = t;
        Ok(())
    }
    fn now(&self) -> Result<i64> {
        Ok(self.state.borrow().clock)
    }
    fn me(&self) -> Result<String> {
        Ok("owner".into())
    }
    fn take_publication_evidence(&self) -> Vec<crate::readiness::Publication> {
        let mut state = self.state.borrow_mut();
        if let Some(publisher) = &state.publisher {
            publisher.take_publication_evidence()
        } else {
            std::mem::take(&mut state.evidence)
        }
    }
    fn defer_publication(&self, mode: &str) -> bool {
        mode == "pr" && self.state.borrow().publish.starts_with("queue-")
    }
    fn cancel_publication(&self, n: u64, sha: &str) -> Result<()> {
        let mut state = self.state.borrow_mut();
        state.cancelled_publications.push((n, sha.into()));
        if state.fail_publication_cancel {
            Err("queue cancellation unknown".into())
        } else {
            Ok(())
        }
    }
    fn close_task_prs(&self, n: u64, reason: &str) -> Result<Vec<u64>> {
        let mut state = self.state.borrow_mut();
        state.closed_task_prs.push((n, reason.into()));
        if state.fail_task_prs {
            Err("PR state unknown".into())
        } else {
            Ok(vec![])
        }
    }

    fn resume_publication(
        &self,
        n: u64,
        sha: &str,
        mode: &str,
        title: &str,
        task_line: &str,
        root: &std::path::Path,
        enqueued: bool,
    ) -> Publish {
        if self.state.borrow().publisher.is_none() {
            return self.publish(n, sha, mode, title, task_line);
        }
        self.state.borrow_mut().publications += 1;
        let state = self.state.borrow();
        state
            .publisher
            .as_ref()
            .unwrap()
            .resume_publication(n, sha, mode, title, task_line, root, enqueued)
    }
    fn publish(&self, n: u64, sha: &str, mode: &str, title: &str, task_line: &str) -> Publish {
        self.state.borrow_mut().publications += 1;
        if let Some(p) = &self.state.borrow().publisher {
            return p.publish(n, sha, mode, title, task_line);
        }
        match self.state.borrow().publish.as_str() {
            "pending" => Publish::Pending("CI pending".into()),
            "failed" => Publish::Failed("CI failed".into()),
            "queue-admitted" => Publish::Enqueued("MQ_1".into()),
            "queue-waiting" => Publish::Pending("merge_group CI pending".into()),
            "manual-merge" => Publish::Pending(format!("PR #7 {MANUAL_MERGE}: invented plan gap; merge it: https://github.com/o/r/pull/7")),
            "queue-refused" => Publish::Pending("merge queue did not admit PR #7: Required status check build is in progress; CI build pending".into()),
            "queue-conflict" => Publish::MainMoved("queue merge conflict".into()),
            "queue-blocked" => Publish::Blocked("PR #7 merged, merge_group CI tests failure".into()),
            "superseded" => Publish::Superseded { reason: "no open taskq-1 PR at result; merged taskq-1 PRs: #86 at ddddddd".into(), merged: "#86 at ddddddd".into() },
            _ => Publish::Done(format!("merged {sha}")),
        }
    }
    fn url(&self, n: u64) -> String {
        format!("https://board/{n}")
    }
}
