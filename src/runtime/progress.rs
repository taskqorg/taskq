//! R6/R12: bounded advisory observation; never feeds execution release or admission.
use super::NativeHistory;
use agentq_runtime::agent::Event;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

#[derive(Default)]
pub struct Reader {
    history: NativeHistory,
    tools: BTreeMap<String, (i64, Option<i64>)>,
    last: Option<i64>,
    seen: bool,
    error: Option<&'static str>,
}

pub fn unknown(reason: &str) -> Value {
    json!({"source":"native execution events","status":"unknown","last_event":null,"reason":reason})
}

/// Host metadata is optional observation, never an implicit healthy fallback for corrupt data.
pub fn valid(p: &Value) -> bool {
    let observed = p["observed"].as_i64().filter(|t| *t > 0);
    let time = |k: &str| {
        p[k].is_null()
            || p[k]
                .as_i64()
                .zip(observed)
                .is_some_and(|(t, now)| t > 0 && t <= now)
    };
    let known = matches!(p["status"].as_str(), Some("quiet" | "active" | "overdue"));
    p["source"] == "native execution events"
        && p["reason"].as_str().is_some_and(|s| {
            !s.is_empty() && s.chars().count() <= 160 && !s.chars().any(char::is_control)
        })
        && (p["status"] == "unknown"
            || (known
                && observed.is_some()
                && time("last_event")
                && time("tool_since")
                && (p["status"] != "overdue"
                    || (p["overdue_since"].is_i64() && time("overdue_since")))
                && p["pending_tools"].as_u64().is_some_and(|n| {
                    n <= 128
                        && if p["status"] == "quiet" {
                            n == 0 && p["last_event"].is_i64()
                        } else {
                            n > 0 && p["tool_since"].is_i64()
                        }
                })))
}

impl Reader {
    pub fn read(
        &mut self,
        path: &Path,
        start: u64,
        rt: &str,
        mut clock: impl FnMut() -> i64,
        poll: u64,
    ) -> Value {
        let modified = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .map_err(|_| "native progress source unreadable")
            .and_then(|at| {
                at.duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .map_err(|_| "native progress time invalid")
            });
        let mut now = clock();
        let result = (|| {
            let modified = modified?;
            if modified > now || modified <= 0 {
                return Err("native progress time is future or unknown");
            }
            let agent = super::agent(rt).map_err(|_| "native progress adapter unavailable")?;
            let mut at = self.history.offset;
            let mut lines = Vec::new();
            let complete = self.history.lines(path, |line| {
                if at >= start && line.trim_start().starts_with('{') {
                    let v = serde_json::from_str::<Value>(line)
                        .map_err(|_| "native progress record invalid".to_string())?;
                    lines.push((v["type"] == "taskq.input", line.to_string()));
                }
                at += line.len() as u64;
                Ok(())
            });
            // A hot log may append across a second boundary during the bounded scan.
            now = clock();
            for (input, line) in lines {
                for record in agent.log_records(&line) {
                    let at = match record.time.observed(now) {
                        Ok(Some(t)) if t > 0 => t,
                        Ok(None) if modified <= now => modified,
                        _ => {
                            self.error = Some("native progress timestamp invalid or future");
                            continue;
                        }
                    };
                    if input {
                        self.reset();
                    }
                    self.apply(&record.events, at);
                }
            }
            if !complete.map_err(|_| "native progress read failed, changed or incomplete")? {
                return Err("native progress scan pending");
            }
            if let Some(reason) = self.error {
                return Err(reason);
            }
            if !self.seen {
                return Err("no supported native progress event");
            }
            Ok(())
        })();
        let mut value = match result {
            Err(reason) => unknown(reason),
            Ok(()) => self.value(now, poll),
        };
        value["observed"] = json!(now);
        value
    }

    /// A turn boundary: pending tools and earlier progress no longer apply.
    fn reset(&mut self) {
        self.tools.clear();
        self.last = None;
        self.seen = false;
        self.error = None;
    }

    fn apply(&mut self, events: &[Event], at: i64) {
        for event in events {
            match event {
                Event::TurnStart => self.reset(),
                Event::ToolStart { id, timeout_ms, .. } => match id {
                    Some(id) => self.tool(
                        id.clone(),
                        true,
                        at,
                        timeout_ms.and_then(|ms| i64::try_from(ms.div_ceil(1000)).ok()),
                    ),
                    None => self.error = Some("native tool identity or state unsupported"),
                },
                Event::ToolEnd { id, .. } => match id {
                    Some(id) => self.tool(id.clone(), false, at, None),
                    None => self.error = Some("native tool identity or state unsupported"),
                },
                Event::Message { .. } | Event::Progress => self.completed(at),
                Event::TurnEnd { .. } => {
                    self.tools.clear();
                    self.error = None;
                    self.completed(at);
                }
                Event::Unsupported { reason } => self.error = Some(reason),
                _ => {}
            }
        }
    }

    fn completed(&mut self, at: i64) {
        self.last = Some(at);
        self.seen = true;
    }
    fn tool(&mut self, id: String, active: bool, at: i64, timeout: Option<i64>) {
        self.seen = true;
        if id.len() > 256 || (active && self.tools.len() >= 128 && !self.tools.contains_key(&id)) {
            self.error = Some("native pending tool identity bound exceeded");
            return;
        }
        if active {
            self.tools.entry(id).or_insert((at, timeout));
        } else {
            self.tools.remove(&id);
            self.completed(at);
        }
    }
    fn value(&self, now: i64, poll: u64) -> Value {
        let overdue_since = self
            .tools
            .values()
            .filter_map(|(at, timeout)| {
                let deadline = timeout.map_or_else(
                    || (*at).max(self.last.unwrap_or(*at)).saturating_add(900),
                    |s| {
                        at.saturating_add(s)
                            .saturating_add((poll.saturating_mul(2)).min(i64::MAX as u64) as i64)
                    },
                );
                (now > deadline).then_some(deadline)
            })
            .min();
        let overdue = overdue_since.is_some();
        let status = if overdue {
            "overdue"
        } else if self.tools.is_empty() {
            "quiet"
        } else {
            "active"
        };
        json!({"source":"native execution events","clock":"native timestamp or append mtime lower bound","status":status,"last_event":self.last,
            "deadline":self.tools.values().filter_map(|(at,s)|s.map(|s|at.saturating_add(s))).min(),"overdue_since":overdue_since,"tool_since":self.tools.values().map(|(at,_)|at).min(),"pending_tools":self.tools.len(),
            "reason":if overdue {"tool progress overdue; preserve execution and inspect owning session"} else if self.tools.is_empty() {"no pending tool; quiet work is not failure"} else {"tool pending within observation deadline"}})
    }
}

#[cfg(test)]
#[path = "progress_tests.rs"]
mod tests;
