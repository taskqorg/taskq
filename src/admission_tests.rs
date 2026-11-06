use super::*;

fn me(held: usize) -> Entry {
    Entry {
        version: "4.2.0".into(),
        at: 0,
        held: (0..held as u64).map(|n| (n, n, "claude".into())).collect(),
        requested: vec![],
    }
}

fn want(max: Option<u32>, sample_at: i64, old: &[String]) -> Want<'_> {
    Want {
        max,
        sample_at: Some(sample_at),
        old,
        res: (9, 0, "claude".into()),
    }
}

// Chain: § 3 host admission. Risk: two projects spend the same headroom, or a cap or old service is bypassed.
#[test]
fn admits_holds_at_cap_releases_and_refuses_old_services() {
    let mut h = Host::default();
    // admit: the reservation is held and the host launch time moves
    assert_eq!(
        decide(&mut h, "/a", me(0), Some(want(Some(2), 50, &[])), 100),
        Ok(())
    );
    assert_eq!((h.launched, used(&h)), (100, 1));
    // another project in the same 30 s and before a newer sample: requested, not held
    assert_eq!(
        decide(&mut h, "/b", me(0), Some(want(Some(2), 120, &[])), 110)
            .unwrap_err()
            .0,
        "spacing"
    );
    assert_eq!(
        decide(&mut h, "/b", me(0), Some(want(Some(2), 90, &[])), 140)
            .unwrap_err()
            .0,
        "spacing"
    );
    assert_eq!(h.services["/b"].requested.len(), 1);
    // hold at the host cap: /a holds 1, /b holds 1, the next waits
    assert_eq!(
        decide(&mut h, "/b", me(1), Some(want(Some(2), 150, &[])), 140)
            .unwrap_err()
            .0,
        "max"
    );
    // release: /a's child exited, its entry rewritten without it; then a stopped service's entry goes stale
    decide(&mut h, "/a", me(0), None, 141).unwrap();
    assert_eq!(
        decide(&mut h, "/b", me(1), Some(want(Some(2), 150, &[])), 145),
        Ok(())
    );
    assert_eq!(used(&h), 2);
    decide(&mut h, "/b", me(0), None, 146).unwrap();
    assert_eq!(decide(&mut h, "/c", me(0), None, 146 + STALE + 1), Ok(()));
    assert_eq!(
        h.services.keys().collect::<Vec<_>>(),
        ["/c"],
        "stale entries released"
    );
    // a fresh older service on the host refuses every Auto launch
    let old = ["p 4.1.11".to_string()];
    assert_eq!(
        decide(&mut h, "/c", me(0), Some(want(None, 400, &old)), 400)
            .unwrap_err()
            .0,
        "version"
    );
    assert!(
        coreq::update::newer(MIN_SERVICE, "4.1.11")
            && !coreq::update::newer(MIN_SERVICE, "4.2.0-dev.1760000000")
    );
}

// #2210, taskq.md § 8 item 1. Risk: a dev build X.Y.Z-yymmdd-N reads as a downgrade, so `current` never moves.
#[test]
fn dev_builds_order_by_day_then_count() {
    let order = [
        "4.6.59",
        "4.6.60-dev.1793639858",
        "4.6.60-261102-0",
        "4.6.60-261102-1",
        "4.6.60-261102-9",
        "4.6.60-261102-10",
        "4.6.60-261103-0",
        "4.6.60",
    ];
    for w in order.windows(2) {
        assert!(
            coreq::update::newer(w[1], w[0]) && !coreq::update::newer(w[0], w[1]),
            "{} > {}",
            w[1],
            w[0]
        );
    }
}

// Chain: § 3 unknown host data. Risk: a corrupt or missing sample admits.
#[test]
fn refuses_on_unknown_and_rebuilds_the_file() {
    let mut h = Host::default();
    let none = Want {
        max: None,
        sample_at: None,
        old: &[],
        res: (1, 0, "codex".into()),
    };
    assert_eq!(
        decide(&mut h, "/a", me(0), Some(none), 100).unwrap_err().0,
        "spacing",
        "no sample: refused"
    );
    let dir = std::env::temp_dir().join(format!("taskq-adm-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("admission.json"), "{not json").unwrap();
    let (r, used) = sync(&dir, "/a", me(1), Some(want(None, 100, &[])), 100);
    assert_eq!(
        (r.unwrap_err().0, used),
        ("host", Some(1)),
        "unreadable file: refused, rebuilt from this entry"
    );
    let (r, _) = sync(&dir, "/a", me(1), Some(want(None, 100, &[])), 100);
    assert_eq!(r, Ok(()));
    let _ = fs::remove_dir_all(&dir);
}
