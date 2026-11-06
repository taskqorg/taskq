//! I3 / UX30-21: a rebuildable projection of first queue admissions, sourced only from board label events.
use crate::board::Board;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const MONTH: i64 = 30 * 86400;
const EVENT_READS: usize = 5;
/// #2083: the longest between admission scans of every issue (§ 4 budget).
const FULL_SCAN: i64 = 1800;
/// #2352: seconds between two scans while none is unresolved; each pass of each project billed every task change once more.
const READ_EVERY: std::time::Duration = std::time::Duration::from_secs(180);

/// Observation stays off the service's heartbeat/admission path, including a slow or unavailable board.
pub struct Reader {
    request: std::sync::mpsc::SyncSender<()>,
    response: std::sync::mpsc::Receiver<Value>,
    busy: bool,
    value: Value,
    every: std::time::Duration,
    asked: Option<std::time::Instant>,
}
impl Reader {
    pub fn new(cfg: crate::service::Config, schema: Option<String>, identity: &str) -> Self {
        let value = crate::data::json(&cfg.root.join(".taskq/status.json"))
            .ok()
            .flatten()
            .map(|v| v["incoming"].clone())
            .filter(|v| valid(v, identity))
            .unwrap_or(Value::Null);
        let seed = value.clone();
        let identity = identity.to_string();
        let (request, requests) = std::sync::mpsc::sync_channel(1);
        let (responses, response) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let board = cfg.board.open(
                &cfg.web_host,
                &cfg.repo,
                schema.as_deref(),
                &crate::platform::tool(&cfg.install),
            );
            let mut history = History::seed(&seed, &identity);
            while let Ok(()) = requests.recv() {
                let value = match &board {
                    Ok(board) => {
                        match board.now() {
                            Ok(now) => history.refresh(board.as_ref(), now),
                            Err(reason) => history.missing = vec![reason.to_string()],
                        }
                        history.json(&board.url(0))
                    }
                    Err(reason) => {
                        history.missing = vec![reason.to_string()];
                        history.json(&identity)
                    }
                };
                if responses.send(value).is_err() {
                    break;
                }
            }
        });
        Self {
            request,
            response,
            busy: false,
            value,
            every: READ_EVERY,
            asked: None,
        }
    }

    pub fn tick(&mut self) -> Value {
        match self.response.try_recv() {
            Ok(value) => {
                self.value = value;
                self.busy = false;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if self.value["schema"] == 1 {
                    self.value["missing"] = json!(["admission reader exited"]);
                } else {
                    self.value = json!({"reason":"admission reader exited"});
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        // An unresolved source keeps reading each pass: the event reads of a pass are bounded.
        let due = self.asked.is_none_or(|t| t.elapsed() >= self.every)
            || self.value["missing"]
                .as_array()
                .is_some_and(|m| !m.is_empty());
        if !self.busy && due && self.request.try_send(()).is_ok() {
            self.busy = true;
            self.asked = Some(std::time::Instant::now());
        }
        self.value.clone()
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct History {
    // Updated issue and its last verified first admission; no task state or launch authority.
    seen: BTreeMap<u64, (i64, Option<(u64, i64)>)>,
    // Issue update, current task eligibility, last event read (0: unvisited); all rebuildable observations.
    pending: BTreeMap<u64, (i64, bool, i64)>,
    // Issue update and reason of a complete history read that failed; read again only after the issue updates.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    failed: BTreeMap<u64, (i64, String)>,
    listed: Option<i64>,
    // Newest non-heartbeat issue update a scan returned: the `taskq` scan asks from it, so an unchanged board repeats one
    // URL and answers 304 (#1985, #2083).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    newest: Option<i64>,
    // Board time of the last scan of every issue (#2083): closed, label-removed and never-admitted issues, at least every
    // FULL_SCAN; heartbeat writes change that reply, so each pass between asks for `taskq` issues only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    full: Option<i64>,
    through: Option<i64>,
    start: Option<i64>,
    missing: Vec<String>,
}

impl History {
    fn seed(v: &Value, board: &str) -> Self {
        if !valid(v, board) {
            return Self::default();
        }
        if let Ok(h) = serde_json::from_value::<Self>(v["cache"].clone()) {
            if h.json(board) == *v
                && h.listed
                    .is_some_and(|t| h.through.is_none_or(|end| end <= t))
                && h.seen.iter().all(|(n, (t, _))| *n > 0 && *t > 0)
                && h.pending
                    .iter()
                    .all(|(n, (t, _, tried))| *n > 0 && *t > 0 && *tried >= 0)
                && h.failed.iter().all(|(n, (t, _))| *n > 0 && *t > 0)
            {
                return h;
            }
        }
        // Older projections retain verified arrivals and coverage, but lack negative observations or a scan cursor.
        let mut h = Self {
            start: v["start"].as_i64(),
            through: v["end"].as_i64(),
            missing: v["missing"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap().to_string())
                .collect(),
            ..Self::default()
        };
        for e in v["events"].as_array().unwrap() {
            let at = e[0].as_i64().unwrap();
            h.seen.insert(
                e[3].as_u64().unwrap(),
                (at, Some((e[2].as_str().unwrap().parse().unwrap(), at))),
            );
        }
        h
    }

    pub fn refresh(&mut self, board: &dyn Board, now: i64) {
        let floor = now - MONTH;
        if self.listed.is_some_and(|t| t > now) {
            self.missing = vec!["admission scan clock moved backwards".into()];
            return;
        }
        // The newest seen update is never after the last scan, so asking from it still covers every later update.
        let after = |t: Option<i64>| t.map_or(floor, |t| (t - 30).max(floor));
        let full = self.full.is_none_or(|t| now - t >= FULL_SCAN || t > now);
        let scan = if full {
            board.issues(Some(after(self.full.or(self.newest).or(self.listed))))
        } else {
            board.tasks(after(self.newest.or(self.listed)))
        };
        let issues = match scan {
            Ok(issues) => issues,
            Err(e) => {
                self.missing = vec![format!("admission scan: {e}")];
                return;
            }
        };
        if full {
            self.full = Some(now);
        }
        self.newest = issues
            .iter()
            .filter(|i| !crate::service::is_host(i))
            .map(|i| i.updated)
            .filter(|t| *t <= now)
            .chain(self.newest)
            .max();
        for issue in issues
            .iter()
            .filter(|i| i.trusted && !crate::service::is_host(i))
        {
            if let Some((updated, event)) = self.seen.get_mut(&issue.number) {
                // A settled earliest admission is immutable; comments and later label changes cannot create another.
                if event.is_some() || *updated == issue.updated {
                    *updated = issue.updated;
                    continue;
                }
            }
            if self
                .failed
                .get(&issue.number)
                .is_some_and(|(updated, _)| *updated == issue.updated)
            {
                continue;
            }
            self.failed.remove(&issue.number);
            let pending = self.pending.entry(issue.number).or_insert((
                issue.updated,
                crate::service::is_task(issue),
                0,
            ));
            pending.0 = issue.updated;
            pending.1 = crate::service::is_task(issue);
        }
        self.listed = Some(now);
        self.seen.retain(|_, (updated, event)| {
            *updated >= floor || event.is_some_and(|(_, at)| at >= floor)
        });
        self.pending.retain(|_, (updated, _, _)| *updated >= floor);
        self.failed.retain(|_, (updated, _)| *updated >= floor);
        let mut pending: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, (updated, _, _))| now >= *updated + 30)
            .map(|(n, (_, _, tried))| (*tried, *n))
            .collect();
        pending.sort_unstable();
        let mut missing = vec![];
        for (_, n) in pending.into_iter().take(EVENT_READS) {
            let (updated, task, tried) = self.pending.get_mut(&n).unwrap();
            *tried = now;
            match admission(board, n, *task).and_then(|event| {
                if event.is_some_and(|(_, at)| at > now) {
                    Err("first-admission time is in the future".into())
                } else {
                    Ok(event)
                }
            }) {
                Ok(event) => {
                    self.seen.insert(n, (*updated, event));
                    self.pending.remove(&n);
                }
                Err(e) if SETTLED.contains(&e.as_str()) => {
                    self.failed.insert(n, (*updated, e));
                    self.pending.remove(&n);
                }
                Err(e) => missing.push(format!("#{n}: {e}")),
            }
        }
        // A failed source keeps coverage incomplete until its issue updates or leaves the month.
        missing.extend(self.failed.iter().map(|(n, (_, e))| format!("#{n}: {e}")));
        // An unresolved source must be retried, including after its issue leaves the update overlap.
        if self.pending.is_empty() && self.failed.is_empty() {
            self.start = Some(self.start.unwrap_or(floor).max(floor));
            // #2083: a scan of `taskq` issues proves nothing of other issues; coverage ends at the last scan of every issue
            self.through = self.full.map(|t| t.min(now));
        } else if !self.pending.is_empty() {
            missing.push(format!("{} admission sources pending", self.pending.len()));
        }
        self.missing = missing;
    }

    pub fn json(&self, board: &str) -> Value {
        let events: Vec<_> = self
            .seen
            .iter()
            .filter_map(|(n, (_, event))| {
                event.map(|(id, at)| json!([at, "new", id.to_string(), n]))
            })
            .collect();
        json!({"schema":1, "board":board, "start":self.start, "end":self.through, "events":events, "missing":self.missing, "cache":self})
    }
}

/// A status cache with malformed provenance cannot establish a complete zero.
pub fn valid(v: &Value, board: &str) -> bool {
    v["schema"] == 1
        && v["board"] == board
        && v["missing"]
            .as_array()
            .is_some_and(|m| m.iter().all(Value::is_string))
        && v["events"].as_array().is_some_and(|events| {
            events.iter().all(|e| {
                e.as_array().is_some_and(|a| a.len() == 4)
                    && e[0].as_i64().is_some_and(|at| at > 0)
                    && e[1] == "new"
                    && e[2]
                        .as_str()
                        .and_then(|id| id.parse::<u64>().ok())
                        .is_some_and(|id| id > 0)
                    && e[3].as_u64().is_some_and(|n| n > 0)
            })
        })
        && (v["start"].is_null() && v["end"].is_null()
            || v["start"]
                .as_i64()
                .zip(v["end"].as_i64())
                .is_some_and(|(a, b)| a <= b))
}

/// Malformed records a complete history read gives again until its issue updates (§ 4 admission records); other failures retry.
const SETTLED: [&str; 3] = [
    "admission ID unavailable",
    "admission time unavailable",
    "invalid first-admission provenance",
];

fn admission(board: &dyn Board, n: u64, task: bool) -> Result<Option<(u64, i64)>, String> {
    let event = board.first_admission(n)?;
    if event.is_some_and(|(id, at)| id == 0 || at <= 0) {
        return Err("invalid first-admission provenance".into());
    }
    if event.is_none() && task {
        return Err("first-admission history unavailable".into());
    }
    Ok(event)
}

#[cfg(test)]
#[path = "incoming_tests.rs"]
mod tests;
