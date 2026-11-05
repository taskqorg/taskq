use super::*;
// Chain: §4 Balancing (#2189 A3/A4/A7): protect weighted selection, quota exclusion and legacy routing.
#[test]
fn weighted_runtime_selection() {
    let weights = BTreeMap::from([("claude".into(), 2), ("codex".into(), 1)]);
    let mut load = BTreeMap::from([
        (
            "claude".into(),
            Load {
                running: 2,
                free: 3,
                headroom: None,
            },
        ),
        (
            "codex".into(),
            Load {
                running: 2,
                free: 3,
                headroom: None,
            },
        ),
    ]);
    let pick = |weights: &Weights,
                load: &BTreeMap<String, Load>,
                labels: &[String],
                hold: &str,
                reviewer| {
        select(
            labels,
            reviewer,
            "codex",
            false,
            weights,
            load,
            |_| false,
            |rt| {
                if rt == hold {
                    Err(("usage-limit", format!("{rt} quota hold")))
                } else {
                    Ok(rt.into())
                }
            },
        )
        .unwrap()
    };
    for review in [false, true] {
        assert_eq!(pick(&weights, &load, &[], "", review).0, "claude");
        load.get_mut("claude").unwrap().headroom = Some(0.1);
        assert_eq!(pick(&weights, &load, &[], "", review).0, "codex");
        assert_eq!(pick(&weights, &load, &[], "codex", review).0, "claude");
        assert_eq!(
            pick(&weights, &load, &["run-claude".into()], "", review).0,
            "claude"
        );
        assert_eq!(pick(&Weights::new(), &load, &[], "", review).0, "codex");
        assert!(
            select(
                &["run-codex".into()],
                review,
                "claude",
                false,
                &weights,
                &load,
                |_| false,
                |_| Err(("usage-limit", "held".into()))
            )
            .is_err()
        );
        load.get_mut("claude").unwrap().headroom = None;
    }
    load.values_mut().for_each(|l| l.running = 0);
    load.get_mut("claude").unwrap().headroom = Some(0.1);
    assert_eq!(
        pick(&weights, &load, &[], "", false).0,
        "codex",
        "empty runtimes still honor headroom"
    );
    assert_eq!(
        super::weights(&serde_json::json!({"weights":{"antigravity":0}})).unwrap()["antigravity"],
        0
    );
    for v in [
        serde_json::json!({"weights":null}),
        serde_json::json!({"weights":{"bogus":1}}),
        serde_json::json!({"weights":{"claude":1.5}}),
    ] {
        assert!(super::weights(&v).is_err());
    }
}

// #2240 A2, §4 Balancing: weight 0 excludes a runtime from unpinned work and review, fallback included;
// a run-* pin still takes it. Protects against a zero-weight runtime being picked when others are unavailable.
#[test]
fn zero_weight_runtime_only_by_pin() {
    let weights = BTreeMap::from([
        ("claude".into(), 1),
        ("codex".into(), 1),
        ("antigravity".into(), 0),
    ]);
    let load: BTreeMap<String, Load> = ["claude", "codex", "antigravity"]
        .map(|rt| {
            (
                rt.into(),
                Load {
                    running: 0,
                    free: 3,
                    headroom: None,
                },
            )
        })
        .into();
    let busy = |rt: &str| {
        if rt == "antigravity" {
            Ok(rt.into())
        } else {
            Err(("capacity", format!("{rt} full")))
        }
    };
    for review in [false, true] {
        for default in ["antigravity", "claude"] {
            let err = select(
                &[],
                review,
                default,
                false,
                &weights,
                &load,
                |_| false,
                busy,
            )
            .unwrap_err();
            assert!(
                !err.1.contains("antigravity"),
                "weight 0 is never a fallback: {err:?}"
            );
            assert_eq!(
                select(
                    &["prefer-antigravity".into()],
                    review,
                    default,
                    false,
                    &weights,
                    &load,
                    |_| false,
                    |rt| Ok(rt.into())
                )
                .unwrap()
                .0,
                "claude"
            );
        }
        assert_eq!(
            select(
                &["run-antigravity".into()],
                review,
                "claude",
                false,
                &weights,
                &load,
                |_| false,
                busy
            )
            .unwrap()
            .0,
            "antigravity"
        );
    }
    let only = BTreeMap::from([("antigravity".into(), 0)]);
    assert_eq!(
        select(
            &[],
            false,
            "antigravity",
            false,
            &only,
            &load,
            |_| false,
            |rt| Ok(rt.into())
        )
        .unwrap()
        .0,
        "claude",
        "unweighted runtimes keep fallback"
    );
}

// #2189 A5/A7, §4 Balancing: ties rotate and spare weighted capacity wins.
#[test]
fn weighted_host_spread() {
    let hosts = vec![("mac".into(), 2.0), ("win".into(), 2.0)];
    assert_eq!(host(hosts.clone(), None).unwrap().0, "mac");
    assert_eq!(host(hosts.clone(), Some("mac")).unwrap().0, "win");
    assert_eq!(host(hosts, Some("win")).unwrap().0, "mac");
    assert_eq!(
        host(vec![("mac".into(), 1.0), ("win".into(), 2.0)], Some("win"))
            .unwrap()
            .0,
        "win"
    );
}
