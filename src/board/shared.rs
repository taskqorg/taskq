//! R4: one GitHub poll, with a separately published snapshot for other host readers.
use super::*;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, OnceLock, TryLockError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Instant, SystemTime},
};

#[derive(Default)]
struct Poll {
    open: Option<github::Open>,
    read: Option<Instant>,
    history: Option<(Instant, i64, i64)>,
    interval: Option<std::time::Duration>,
    epoch: u64,
    full: u64,
    pokes: BTreeMap<std::path::PathBuf, Option<SystemTime>>,
}
#[derive(Clone, Default)]
struct Snapshot {
    ready: bool,
    history_since: Option<i64>,
    rows: BTreeMap<u64, Issue>,
    confirmed: BTreeMap<u64, Issue>,
    clock: i64,
    listed: Option<i64>,
}
#[derive(Default)]
struct Shared {
    poll: Mutex<Poll>,
    snapshot: Mutex<Snapshot>,
    epoch: AtomicU64,
    full: AtomicU64,
}
fn inventory(api: &Api) -> Result<Arc<Shared>> {
    static BOARDS: OnceLock<Mutex<BTreeMap<String, Arc<Shared>>>> = OnceLock::new();
    let key = crate::requests::key(&api.base, &api.credentials.borrow().auth);
    Ok(BOARDS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "shared board registry poisoned")?
        .entry(key)
        .or_default()
        .clone())
}
pub(super) fn forget(api: &Api, full: bool) {
    if let Ok(shared) = inventory(api) {
        shared.epoch.fetch_add(1, Ordering::AcqRel);
        if full {
            shared.full.fetch_add(1, Ordering::AcqRel);
        }
    }
}
pub(super) fn clear_confirmation(api: &Api, number: u64) {
    if let Ok(shared) = inventory(api)
        && let Ok(mut snapshot) = shared.snapshot.lock()
    {
        snapshot.confirmed.remove(&number);
    }
}
pub(super) fn patch(api: &Api, issue: &Issue) {
    if let Ok(shared) = inventory(api)
        && let Ok(mut snapshot) = shared.snapshot.lock()
    {
        if issue.trusted && crate::service::is_host(issue) {
            snapshot.confirmed.insert(issue.number, issue.clone());
        }
        if snapshot.rows.contains_key(&issue.number) {
            snapshot.rows.insert(issue.number, issue.clone());
        }
    }
}
fn read(shared: &Shared) -> Result<Snapshot> {
    let snapshot = shared
        .snapshot
        .lock()
        .map_err(|_| "shared board snapshot poisoned")?;
    if !snapshot.ready {
        return Err("shared board snapshot unavailable".into());
    }
    Ok(snapshot.clone())
}
fn rows(api: &Api, snapshot: Snapshot, since: Option<i64>, tasks: bool) -> Vec<Issue> {
    *api.clock.borrow_mut() = snapshot.clock;
    api.proxy_listed
        .set(snapshot.listed.or(Some(snapshot.clock)));
    snapshot
        .rows
        .into_values()
        .filter(|i| since.map_or(i.open, |since| i.updated >= since))
        .filter(|i| !tasks || i.labels.iter().any(|l| l == crate::service::TASK_LABEL))
        .collect()
}
/// A local reader never waits for the network poll or initiates one.
pub(super) fn snapshot(api: &Api, since: Option<i64>, tasks: bool) -> Result<Vec<Issue>> {
    let shared = inventory(api)?;
    let saved = read(&shared)?;
    if since.is_some_and(|since| saved.history_since.is_none_or(|floor| since < floor)) {
        return Err("shared board historical range unavailable".into());
    }
    Ok(rows(api, saved, since, tasks))
}
pub(super) fn issues(
    board: &github::GitHub,
    since: Option<i64>,
    tasks: bool,
) -> Result<Vec<Issue>> {
    let api = &board.api;
    api.refresh()?;
    api.allowed("GET", &api.base)?;
    let shared = inventory(api)?;
    let mut poll = match shared.poll.try_lock() {
        Ok(poll) => poll,
        Err(TryLockError::WouldBlock) => return snapshot(api, since, tasks),
        Err(TryLockError::Poisoned(_)) => return Err("shared board poll poisoned".into()),
    };
    let interval = api.poll.ok_or("shared board poll interval unavailable")?;
    poll.interval = Some(poll.interval.map_or(interval, |prior| prior.min(interval)));
    let pokes: Vec<_> = api
        .pokes
        .iter()
        .flatten()
        .map(|path| {
            (
                path.clone(),
                std::fs::metadata(path).and_then(|m| m.modified()).ok(),
            )
        })
        .collect();
    let poked = pokes
        .iter()
        .any(|(path, at)| poll.pokes.get(path).is_some_and(|prior| prior != at));
    let history_interval = interval.min(std::time::Duration::from_secs(
        crate::incoming::FULL_SCAN as u64,
    ));
    let history_due = since.is_some_and(|since| {
        poll.history.is_none_or(|(at, floor, _)| {
            since < floor || (!tasks && at.elapsed() >= history_interval)
        })
    });
    let epoch = shared.epoch.load(Ordering::Acquire);
    let full = shared.full.load(Ordering::Acquire);
    if poll
        .read
        .is_none_or(|at| at.elapsed() >= poll.interval.unwrap())
        || poll.epoch != epoch
        || poked
        || poll.full != full
        || history_due
    {
        let began = Instant::now();
        let initial = poll.read.is_none();
        let previous = board.open.replace(poll.open.take());
        let mut changes = vec![];
        let list = board.poll_open(&mut changes, poll.full != full);
        poll.open = board.open.replace(previous);
        let mut list = list?;
        let mut history_since = None;
        if initial || history_due {
            let clock = if *api.clock.borrow() > 0 {
                *api.clock.borrow()
            } else {
                wall()
            };
            let floor = since.unwrap_or_else(|| clock.saturating_sub(crate::incoming::MONTH));
            let floor = poll
                .history
                .map_or(floor, |(_, _, through)| floor.min(through));
            changes.extend(
                api.all(&format!(
                    "/issues?state=all&per_page=100&since={}",
                    stamp(floor)
                ))?
                .iter()
                .filter(|v| v.get("pull_request").is_none())
                .map(github::parse_issue),
            );
            history_since = Some((floor, clock));
            // Include closed task updates in the unchanged probe's watermark.
            if let Some(open) = poll.open.as_mut() {
                for issue in changes
                    .iter()
                    .filter(|i| i.labels.iter().any(|l| l == crate::service::TASK_LABEL))
                {
                    open.known.insert(issue.number, issue.updated);
                }
            }
        }
        let mut saved = shared
            .snapshot
            .lock()
            .map_err(|_| "shared board snapshot poisoned")?;
        let mut next = saved.clone();
        // The confirmed reply can arrive while the poll is in flight; only older list rows are corrected.
        for issue in &mut list {
            if let Some(written) = next.confirmed.get(&issue.number).filter(|written| {
                issue.trusted
                    && crate::service::is_host(issue)
                    && issue.updated > 0
                    && issue.updated < written.updated
            }) {
                *issue = written.clone();
            }
        }
        for issue in &mut list {
            if let Some(changed) = changes
                .iter()
                .find(|changed| changed.number == issue.number && changed.updated >= issue.updated)
            {
                *issue = changed.clone();
            }
        }
        list.retain(|issue| issue.open);
        if let Some(open) = poll.open.as_mut() {
            open.list = list.clone();
        }
        next.rows.retain(|_, issue| !issue.open);
        for issue in list.into_iter().chain(changes) {
            if next
                .rows
                .get(&issue.number)
                .is_none_or(|prior| prior.updated <= issue.updated)
            {
                next.rows.insert(issue.number, issue);
            }
        }
        next.clock = *api.clock.borrow();
        next.listed = poll.open.as_ref().map(|o| o.listed);
        next.ready = true;
        if let Some((floor, through)) = history_since {
            next.history_since = Some(next.history_since.map_or(floor, |prior| prior.min(floor)));
            poll.history = Some((began, next.history_since.unwrap(), through));
        }
        *saved = next;
        poll.read = Some(began);
        poll.epoch = epoch;
        poll.full = full;
        poll.pokes.extend(pokes);
    } else {
        poll.pokes.extend(pokes);
    }
    drop(poll);
    snapshot(api, since, tasks)
}

#[cfg(test)]
mod tests {
    use super::*;
    // Chain: #2431 A4/R4: accepted writes and snapshot readers never wait behind a stalled poll.
    #[test]
    fn shared_2431_invalidation_never_waits_on_a_poll() {
        let api = Api::new(
            "https://fixture.invalid".into(),
            format!("https://fixture.invalid/repos/o/{}", crate::runtime::uuid()),
            "invented",
        );
        let shared = inventory(&api).unwrap();
        let held = shared.poll.lock().unwrap();
        let beat = github::parse_issue(
            &json!({"number":7,"title":"taskq host win","labels":[{"name":"taskq-host"}],"author_association":"OWNER","state":"open","updated_at":stamp(1)}),
        );
        patch(&api, &beat);
        assert!(
            snapshot(&api, None, false).is_err(),
            "a write before the first complete poll cannot publish an empty snapshot"
        );
        {
            let mut state = shared.snapshot.lock().unwrap();
            assert_eq!(state.confirmed[&7].updated, beat.updated);
            state.ready = true;
            state.clock = 1;
        }
        forget(&api, false);
        forget(&api, true);
        assert!(snapshot(&api, None, false).unwrap().is_empty());
        assert_eq!(shared.epoch.load(Ordering::Acquire), 2);
        assert_eq!(shared.full.load(Ordering::Acquire), 1);
        drop(held);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = shared.poll.lock().unwrap();
            panic!("invented poll failure");
        }));
        assert!(
            snapshot(&api, None, false).unwrap().is_empty(),
            "poll poisoning cannot corrupt the complete snapshot"
        );
        assert!(
            issues(&github::GitHub::new(api), None, false)
                .err()
                .unwrap()
                .text()
                .contains("poll poisoned")
        );
    }
    // Chain: #2431 A4: the real cross-process poke invalidates a still-fresh poll, without an in-process epoch.
    #[test]
    fn shared_2431_cross_process_poke_refreshes_the_inventory() {
        let root =
            std::env::temp_dir().join(format!("taskq-shared-poke-{}", crate::runtime::uuid()));
        std::fs::create_dir_all(root.join(".taskq")).unwrap();
        let changed = std::rc::Rc::new(std::cell::Cell::new(false));
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let (state, sent) = (changed.clone(), calls.clone());
        let mut api = Api::new(
            "https://fixture.invalid".into(),
            format!("https://fixture.invalid/repos/o/{}", crate::runtime::uuid()),
            "invented",
        );
        api.poll = Some(std::time::Duration::from_secs(60));
        api.pokes = Some([root.join(".taskq/poke"), root.join("install-poke")]);
        api.transport = Some(Box::new(move |_, url, _| {
            sent.set(sent.get() + 1);
            if url.contains("sort=updated") {
                assert!(
                    url.contains("labels=taskq"),
                    "unlabelled heartbeat query: {url}"
                );
            }
            if let Some(since) = url.split("since=").nth(1) {
                assert!(
                    iso(since).unwrap()
                        >= iso("2026-11-06T00:01:00Z").unwrap() - crate::incoming::MONTH,
                    "bootstrap exceeds the existing month: {url}"
                );
            }
            Ok(Reply{status:200,headers:vec![("Date".into(),"Fri, 06 Nov 2026 00:01:00 GMT".into())],body:json!([{"number":5,"state":"open","labels":[{"name":"taskq"}],"body":if state.get(){"after"}else{"before"},"created_at":"2026-11-06T00:00:00Z","updated_at":if state.get(){"2026-11-06T00:00:01Z"}else{"2026-11-06T00:00:00Z"}}]).to_string(),operation:"read"})
        }));
        let board = github::GitHub::new(api);
        assert_eq!(board.issues(None).unwrap()[0].body, "before");
        let before = calls.get();
        assert_eq!(board.issues(None).unwrap()[0].body, "before");
        assert_eq!(calls.get(), before);
        changed.set(true);
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "board::shared::tests::shared_2431_poke_child",
                "--ignored",
                "--nocapture",
            ])
            .env("TASKQ_SHARED_POKE_ROOT", &root)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(board.issues(None).unwrap()[0].body, "after");
        assert!(calls.get() > before);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "private cross-process poke fixture"]
    fn shared_2431_poke_child() {
        crate::wake::poke(std::path::Path::new(
            &std::env::var_os("TASKQ_SHARED_POKE_ROOT").unwrap(),
        ))
        .unwrap();
    }
}
