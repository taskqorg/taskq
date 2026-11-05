//! Owner weights rank eligible launches; admission and execution authority stay with the service.
use crate::Res;
use std::collections::BTreeMap;

pub type Weights = BTreeMap<String, u32>;

pub fn weights(value: &serde_json::Value) -> Res<Weights> {
    let Some(value) = value.get("weights") else {
        return Ok(Weights::new());
    };
    value
        .as_object()
        .ok_or("weights must be an object")?
        .iter()
        .map(|(rt, n)| {
            let n = n
                .as_u64()
                .filter(|n| {
                    (0..=1_000_000).contains(n)
                        && crate::runtime::CLI_RUNTIMES.contains(&rt.as_str())
                })
                .ok_or("weights need CLI runtime names and integers 0..1000000")?;
            Ok((rt.clone(), n as u32))
        })
        .collect()
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Load {
    pub running: u64,
    pub free: u64,
    pub headroom: Option<f64>,
}

impl Load {
    pub fn effective(&self, weight: u32) -> f64 {
        f64::from(weight) * self.headroom.unwrap_or(1.0).max(0.01)
    }
    pub fn valid(&self) -> bool {
        self.headroom
            .is_none_or(|n| n.is_finite() && (0.0..=1.0).contains(&n))
    }
}

pub fn select(
    labels: &[String],
    reviewer: bool,
    default: &str,
    auto_reroute: bool,
    weights: &Weights,
    load: &BTreeMap<String, Load>,
    disabled: impl Fn(&str) -> bool,
    mut eligible: impl FnMut(&str) -> Result<String, (&'static str, String)>,
) -> Result<(String, Option<String>), (&'static str, String)> {
    if weights.is_empty() || labels.iter().any(|l| l.starts_with("run-")) {
        return crate::labels::select_project(
            labels,
            reviewer,
            default,
            auto_reroute,
            disabled,
            eligible,
        );
    }
    // Weight 0: unpinned work and review never take this runtime, not even as fallback; `run-*` pins returned above.
    let mut candidates = crate::labels::candidates(labels, reviewer, default);
    candidates.retain(|rt| weights.get(rt) != Some(&0));
    if candidates.is_empty() {
        return Err((
            "capacity",
            "every candidate runtime has weight 0; pin one with run-<runtime>".into(),
        ));
    }
    candidates.sort_by(|a, b| {
        let rank = |rt: &str| {
            weights.get(rt).zip(load.get(rt)).map(|(w, l)| {
                let effective = l.effective(*w);
                (l.running as f64 / effective, effective)
            })
        };
        match (rank(a), rank(b)) {
            (Some(a), Some(b)) => a.0.total_cmp(&b.0).then_with(|| b.1.total_cmp(&a.1)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        }
    });
    let mut failures = vec![];
    let mut code = "unknown";
    for rt in candidates {
        match eligible(&rt) {
            Ok(actual) => {
                let reason=weights.get(&rt).zip(load.get(&rt)).map(|(w,l)|format!("weighted pick {rt}: running {}, weight {w}, headroom {}, effective {:.3}",
                    l.running,l.headroom.map_or("not observed".into(),|h|format!("{:.1}%",h*100.0)),l.effective(*w)))
                    .unwrap_or_else(||format!("weighted fallback: selected {rt}"));
                return Ok((
                    actual,
                    Some(if failures.is_empty() {
                        reason
                    } else {
                        format!("{}; {reason}", failures.join("; "))
                    }),
                ));
            }
            Err(e) if e.0 == "unknown" => return Err(e),
            Err(e) => {
                if failures.is_empty() {
                    code = e.0;
                }
                failures.push(e.1);
            }
        }
    }
    Err((code, failures.join("; ")))
}

/// Rotate tied hosts using the existing trusted spawn log; no local scheduler cursor.
pub fn host(mut candidates: Vec<(String, f64)>, last: Option<&str>) -> Option<(String, f64)> {
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let max = candidates.first()?.1;
    candidates.retain(|c| c.1 == max);
    let pick = last
        .and_then(|last| candidates.iter().position(|c| c.0.as_str() > last))
        .unwrap_or(0);
    Some(candidates.swap_remove(pick))
}

#[cfg(test)]
#[path = "balance_tests.rs"]
mod tests;
