//! Owner weights rank eligible launches; admission and execution authority stay with the service.
use std::collections::BTreeMap;
use crate::Res;

pub type Weights = BTreeMap<String, u32>;

pub fn weights(value: &serde_json::Value) -> Res<Weights> {
    let Some(value) = value.get("weights") else { return Ok(Weights::new()) };
    value.as_object().ok_or("weights must be an object")?.iter().map(|(rt,n)| {
        let n = n.as_u64().filter(|n| (1..=1_000_000).contains(n) && crate::runtime::CLI_RUNTIMES.contains(&rt.as_str()))
            .ok_or("weights need CLI runtime names and integers 1..1000000")?;
        Ok((rt.clone(),n as u32))
    }).collect()
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Load {
    pub running: u64,
    pub free: u64,
    pub headroom: Option<f64>,
}

impl Load {
    pub fn effective(&self, weight:u32) -> f64 { f64::from(weight) * self.headroom.unwrap_or(1.0).max(0.01) }
    pub fn valid(&self) -> bool { self.headroom.is_none_or(|n|n.is_finite() && (0.0..=1.0).contains(&n)) }
}

pub fn select(labels:&[String], reviewer:bool, default:&str, auto_reroute:bool, weights:&Weights,
    load:&BTreeMap<String,Load>, disabled:impl Fn(&str)->bool,
    mut eligible:impl FnMut(&str)->Result<String,(&'static str,String)>) -> Result<(String,Option<String>),(&'static str,String)> {
    if weights.is_empty() || labels.iter().any(|l|l.starts_with("run-")) {
        return crate::labels::select_project(labels,reviewer,default,auto_reroute,disabled,eligible);
    }
    let mut candidates=crate::labels::candidates(labels,reviewer,default);
    candidates.sort_by(|a,b| {
        let rank=|rt:&str|weights.get(rt).zip(load.get(rt)).map(|(w,l)| {
            let effective=l.effective(*w);(l.running as f64/effective,effective)
        });
        match (rank(a),rank(b)) {
            (Some(a),Some(b))=>a.0.total_cmp(&b.0).then_with(||b.1.total_cmp(&a.1)),
            (Some(_),None)=>std::cmp::Ordering::Less,
            (None,Some(_))=>std::cmp::Ordering::Greater,
            _=>std::cmp::Ordering::Equal,
        }
    });
    let mut failures=vec![];
    let mut code="unknown";
    for rt in candidates {
        match eligible(&rt) {
            Ok(actual)=>{
                let reason=weights.get(&rt).zip(load.get(&rt)).map(|(w,l)|format!("weighted pick {rt}: running {}, weight {w}, headroom {}, effective {:.3}",
                    l.running,l.headroom.map_or("unknown".into(),|h|format!("{:.1}%",h*100.0)),l.effective(*w)))
                    .unwrap_or_else(||format!("weighted fallback: selected {rt}"));
                return Ok((actual,Some(if failures.is_empty() {reason}else{format!("{}; {reason}",failures.join("; "))})));
            },
            Err(e) if e.0=="unknown"=>return Err(e),
            Err(e)=>{if failures.is_empty() {code=e.0;} failures.push(e.1);},
        }
    }
    Err((code,failures.join("; ")))
}

/// Rotate tied hosts using the existing trusted spawn log; no local scheduler cursor.
pub fn host(mut candidates:Vec<(String,f64)>, last:Option<&str>) -> Option<(String,f64)> {
    candidates.sort_by(|a,b|b.1.total_cmp(&a.1).then_with(||a.0.cmp(&b.0)));
    let max=candidates.first()?.1;
    candidates.retain(|c|c.1==max);
    let pick=last.and_then(|last|candidates.iter().position(|c|c.0.as_str()>last)).unwrap_or(0);
    Some(candidates.swap_remove(pick))
}

#[cfg(test)]
mod tests {
    use super::*;
    // Chain: §4 Balancing (#2189 A3/A4/A7): protect weighted selection, quota exclusion and legacy routing.
    #[test]
    fn weighted_runtime_selection() {
        let weights=BTreeMap::from([("claude".into(),2),("codex".into(),1)]);
        let mut load=BTreeMap::from([("claude".into(),Load{running:2,free:3,headroom:None}),("codex".into(),Load{running:2,free:3,headroom:None})]);
        let pick=|weights:&Weights,load:&BTreeMap<String,Load>,labels:&[String],hold:&str,reviewer|select(labels,reviewer,"codex",false,weights,load,|_|false,
            |rt|if rt==hold {Err(("usage-limit",format!("{rt} quota hold")))}else{Ok(rt.into())}).unwrap();
        for review in [false,true] {
            assert_eq!(pick(&weights,&load,&[],"",review).0,"claude");
            load.get_mut("claude").unwrap().headroom=Some(0.1);
            assert_eq!(pick(&weights,&load,&[],"",review).0,"codex");
            assert_eq!(pick(&weights,&load,&[],"codex",review).0,"claude");
            assert_eq!(pick(&weights,&load,&["run-claude".into()],"",review).0,"claude");
            assert_eq!(pick(&Weights::new(),&load,&[],"",review).0,"codex");
            assert!(select(&["run-codex".into()],review,"claude",false,&weights,&load,|_|false,|_|Err(("usage-limit","held".into()))).is_err());
            load.get_mut("claude").unwrap().headroom=None;
        }
        load.values_mut().for_each(|l|l.running=0);
        load.get_mut("claude").unwrap().headroom=Some(0.1);
        assert_eq!(pick(&weights,&load,&[],"",false).0,"codex","empty runtimes still honor headroom");
        for v in [serde_json::json!({"weights":null}),serde_json::json!({"weights":{"codex":0}}),serde_json::json!({"weights":{"bogus":1}}),serde_json::json!({"weights":{"claude":1.5}})] {assert!(super::weights(&v).is_err());}
    }

    // #2189 A5/A7, §4 Balancing: ties rotate and spare weighted capacity wins.
    #[test]
    fn weighted_host_spread() {
        let hosts=vec![("mac".into(),2.0),("win".into(),2.0)];
        assert_eq!(host(hosts.clone(),None).unwrap().0,"mac");
        assert_eq!(host(hosts.clone(),Some("mac")).unwrap().0,"win");
        assert_eq!(host(hosts,Some("win")).unwrap().0,"mac");
        assert_eq!(host(vec![("mac".into(),1.0),("win".into(),2.0)],Some("win")).unwrap().0,"win");
    }
}
