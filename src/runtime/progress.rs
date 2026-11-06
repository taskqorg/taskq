//! R6/R12: bounded advisory observation; never feeds execution release or admission.
use super::NativeHistory;
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
            let mut at = self.history.offset;
            let mut events = Vec::new();
            let complete = self.history.lines(path, |line| {
                if at >= start && line.trim_start().starts_with('{') {
                    events.push(
                        serde_json::from_str::<Value>(line)
                            .map_err(|_| "native progress record invalid".to_string())?,
                    );
                }
                at += line.len() as u64;
                Ok(())
            });
            // A hot log may append across a second boundary during the bounded scan.
            now = clock();
            for v in events {
                let at = match v.get("timestamp") {
                    Some(Value::String(s)) => crate::time::iso(s).filter(|t| *t > 0 && *t <= now),
                    None => (modified <= now).then_some(modified),
                    _ => None,
                };
                let Some(at) = at else {
                    self.error = Some("native progress timestamp invalid or future");
                    continue;
                };
                self.event(&v, at);
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

    fn event(&mut self, v: &Value, at: i64) {
        let kind = v["type"].as_str().or(v["event"].as_str()).unwrap_or("");
        if matches!(kind, "taskq.input" | "turn.started" | "init") {
            self.tools.clear();
            self.last = None;
            self.seen = false;
            self.error = None;
        }
        let s = &v["step_update"];
        if kind == "step_update" {
            if s["step_type"] == "tool" {
                match (s["step_index"].as_u64(), s["state"].as_str()) {
                    (Some(id), Some(state @ ("ACTIVE" | "DONE" | "ERROR" | "FAILED"))) => self
                        .tool(
                            format!("{}:{id}", s["conversation_id"].as_str().unwrap_or("")),
                            state == "ACTIVE",
                            at,
                            timeout(&s["tool_info"]["parameters"]),
                        ),
                    _ => self.error = Some("native tool identity or state unsupported"),
                }
            } else if s["state"] == "DONE" {
                self.completed(at);
            }
        }
        if matches!(kind, "item.started" | "item.completed") {
            let item = &v["item"];
            if matches!(
                item["type"].as_str(),
                Some("command_execution" | "mcp_tool_call" | "web_search")
            ) {
                if let Some(id) = item["id"].as_str() {
                    self.tool(id.into(), kind == "item.started", at, timeout(item));
                }
            } else if kind == "item.completed" {
                self.completed(at);
            }
        }
        let p = if kind == "response_item" {
            &v["payload"]
        } else {
            v
        };
        match p["type"].as_str() {
            Some("function_call" | "custom_tool_call") => {
                if let Some(id) = p["call_id"].as_str() {
                    let input = p["arguments"]
                        .as_str()
                        .and_then(|s| serde_json::from_str::<Value>(s).ok())
                        .unwrap_or(Value::Null);
                    self.tool(id.into(), true, at, timeout(&input));
                }
            }
            Some("function_call_output" | "custom_tool_call_output") => {
                if let Some(id) = p["call_id"].as_str() {
                    self.tool(id.into(), false, at, None);
                }
            }
            Some("assistant" | "user") => {
                for block in p["message"]["content"].as_array().into_iter().flatten() {
                    match block["type"].as_str() {
                        Some("tool_use") => {
                            if let Some(id) = block["id"].as_str() {
                                self.tool(id.into(), true, at, timeout(&block["input"]));
                            }
                        }
                        Some("tool_result") => {
                            if let Some(id) = block["tool_use_id"].as_str() {
                                self.tool(id.into(), false, at, None);
                            }
                        }
                        Some("text") => self.completed(at),
                        _ => {}
                    }
                }
            }
            Some("message" | "reasoning") => self.completed(at),
            _ => {}
        }
        if (kind == "system" && v["subtype"] == "turn_duration")
            || matches!(kind, "turn.completed" | "turn.failed" | "result")
            || v["event"] == "result"
            || (kind == "event_msg"
                && matches!(
                    v["payload"]["type"].as_str(),
                    Some("task_complete" | "turn_aborted")
                ))
        {
            self.tools.clear();
            self.error = None;
            self.completed(at);
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

fn timeout(v: &Value) -> Option<i64> {
    [
        ("timeout_ms", 1000),
        ("timeout", 1000),
        ("timeout_seconds", 1),
    ]
    .into_iter()
    .find_map(|(k, unit)| {
        v[k].as_u64()
            .filter(|n| *n > 0)
            .and_then(|n| i64::try_from(n.div_ceil(unit)).ok())
    })
}

#[cfg(test)]
#[path = "progress_tests.rs"]
mod tests;
