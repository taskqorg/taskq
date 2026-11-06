//! R4: one rebuildable issue inventory per board/credential, shared by host readers.
use super::*;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

#[derive(Default)]
struct Inventory {
    rows: BTreeMap<u64, Issue>,
    read: Option<Instant>,
    full: Option<Instant>,
    clock: i64,
    interval: Option<std::time::Duration>,
    epoch: u64,
    full_epoch: u64,
}

#[derive(Default)]
struct Shared {
    state: Mutex<Inventory>,
    epoch: AtomicU64,
    full: AtomicU64,
}

fn inventory(api: &Api) -> Result<Arc<Shared>> {
    static BOARDS: OnceLock<Mutex<BTreeMap<String, Arc<Shared>>>> = OnceLock::new();
    let key = crate::requests::key(&api.base, &api.credentials.borrow().auth);
    Ok(BOARDS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "shared board inventory poisoned")?
        .entry(key)
        .or_default()
        .clone())
}

pub(super) fn forget(api: &Api, full: bool) {
    if let Ok(shared) = inventory(api) {
        // Supervision invalidates without waiting behind another reader's HTTP request.
        shared.epoch.fetch_add(1, Ordering::AcqRel);
        if full {
            shared.full.fetch_add(1, Ordering::AcqRel);
        }
    }
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
    let mut state = shared
        .state
        .lock()
        .map_err(|_| "shared board inventory poisoned")?;
    let interval = api.poll.ok_or("shared board poll interval unavailable")?;
    state.interval = Some(state.interval.map_or(interval, |prior| prior.min(interval)));
    let epoch = shared.epoch.load(Ordering::Acquire);
    let full_epoch = shared.full.load(Ordering::Acquire);
    let full = state.full_epoch != full_epoch
        || state
            .full
            .is_none_or(|at| at.elapsed() >= github::OPEN_REUSE);
    let stale = state
        .read
        .is_none_or(|at| at.elapsed() >= state.interval.unwrap());
    if full || stale || state.epoch != epoch {
        // Bootstrap all states once, so a historical reader never needs its own poll or loses a closed task.
        let start = Instant::now();
        let initial = state.clock == 0;
        let url = if initial {
            "/issues?state=all&per_page=100".to_string()
        } else {
            let after = state
                .rows
                .values()
                .map(|issue| issue.updated)
                .filter(|updated| *updated <= state.clock)
                .max()
                .unwrap_or(0);
            format!(
                "/issues?state=all&per_page=100&since={}",
                stamp(after.saturating_sub(30))
            )
        };
        let changes: Vec<_> = api
            .all(&url)?
            .iter()
            .filter(|v| v.get("pull_request").is_none())
            .map(github::parse_issue)
            .collect();
        if full && !initial {
            let open = if api.root == "https://api.github.com" {
                board.open_issues()?
            } else {
                api.all("/issues?state=open&per_page=100")?
                    .iter()
                    .filter(|v| v.get("pull_request").is_none())
                    .map(github::parse_issue)
                    .collect()
            };
            state.rows.retain(|_, issue| !issue.open);
            for issue in open {
                state.rows.insert(issue.number, issue);
            }
        }
        for issue in changes {
            if state
                .rows
                .get(&issue.number)
                .is_none_or(|known| known.updated <= issue.updated)
            {
                state.rows.insert(issue.number, issue);
            }
        }
        if full {
            state.full = Some(start);
        }
        state.epoch = epoch;
        state.full_epoch = full_epoch;
        state.clock = *api.clock.borrow();
        state.read = Some(start);
        board.shared_listed.set(None);
    } else {
        *api.clock.borrow_mut() = state.clock;
        board.shared_listed.set(Some(state.clock));
    }
    Ok(state
        .rows
        .values()
        .filter(|i| match since {
            Some(since) => i.updated >= since,
            None => i.open,
        })
        .filter(|i| !tasks || i.labels.iter().any(|l| l == crate::service::TASK_LABEL))
        .cloned()
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    // Chain: #2431 A4/R4: a stalled poll cannot hold supervision's accepted-write invalidation.
    #[test]
    fn shared_2431_invalidation_never_waits_on_a_poll() {
        let api = Api::new(
            "https://fixture.invalid".into(),
            format!(
                "https://fixture.invalid/repos/owner/{}",
                crate::runtime::uuid()
            ),
            "invented",
        );
        let shared = inventory(&api).unwrap();
        let held = shared.state.lock().unwrap();
        forget(&api, false);
        forget(&api, true);
        assert_eq!(shared.epoch.load(Ordering::Acquire), 2);
        assert_eq!(shared.full.load(Ordering::Acquire), 1);
        drop(held);
    }
}
