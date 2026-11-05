use super::*;
// A3/A4: retention budgets must not deny large full folds or silently bypass warm reuse.
#[test]
fn selected_large_histories_remain_readable_without_uncached_board_reads() {
    let cache = Selected::default();
    let now = std::time::Instant::now();
    let value = json!({"tasks":[{"revision":71,"state":"closed","history":[{"text":"invented ".repeat(256*1024)}]}]});
    assert!(value.to_string().len() > 1024 * 1024);
    assert_eq!(
        cache
            .read("large".into(), None, now, || Ok(value.clone()))
            .unwrap(),
        value
    );
    assert_eq!(
        cache
            .read("large".into(), Some(71), now, || panic!(
                "large warm page repeated board HTTP"
            ))
            .unwrap(),
        value
    );
    let oversized = json!({"tasks":[{"revision":72,"state":"closed","history":[{"text":"x".repeat(8*1024*1024)}]}]});
    assert!(oversized.to_string().len() > 8 * 1024 * 1024);
    assert_eq!(
        cache
            .read("oversized".into(), None, now, || Ok(oversized.clone()))
            .unwrap(),
        oversized
    );
    assert_eq!(
        cache
            .read(
                "oversized".into(),
                Some(72),
                now + std::time::Duration::from_secs(59),
                || panic!("large warm source revalidated HTTP")
            )
            .unwrap(),
        oversized
    );
    assert_eq!(
        cache
            .read(
                "oversized".into(),
                Some(71),
                now + std::time::Duration::from_secs(59),
                || Ok(value.clone())
            )
            .unwrap(),
        value,
        "a changed revision invalidates large sources too"
    );
}
#[test]
fn selected_byte_budget_evicts_idle_sources_without_denying_a_larger_source() {
    let cache = std::sync::Arc::new(Selected::default());
    let now = std::time::Instant::now();
    let value = json!({"tasks":[{"revision":71,"state":"closed","history":[{"text":"x".repeat(9*1024*1024)}]}]});
    for n in 0..8 {
        cache
            .read(n.to_string(), None, now, || Ok(value.clone()))
            .unwrap();
    }
    {
        let entries = cache.entries.lock().unwrap();
        assert_eq!(entries.len(), 7);
        assert!(entries.iter().map(|(_, _, n)| n).sum::<usize>() <= 64 * 1024 * 1024);
        assert!(entries.iter().all(|(k, _, _)| k != "0"));
    }
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let c = cache.clone();
    let busy = std::thread::spawn(move || {
        c.read("busy".into(), None, now, || {
            entered.send(()).unwrap();
            wait.recv().unwrap();
            Ok(json!({"tasks":[{"state":"closed","revision":74}]}))
        })
    });
    ready.recv().unwrap();
    let larger = json!({"tasks":[{"revision":72,"state":"closed","history":[{"text":"x".repeat(65*1024*1024)}]}]});
    cache
        .read("larger".into(), None, now, || Ok(larger))
        .unwrap();
    assert_eq!(
        cache.entries.lock().unwrap().len(),
        2,
        "the in-flight source cannot be evicted"
    );
    assert_eq!(
        cache
            .read("larger".into(), Some(72), now, || panic!(
                "larger warm source repeated HTTP"
            ))
            .unwrap()["tasks"][0]["revision"],
        72
    );
    release.send(()).unwrap();
    busy.join().unwrap().unwrap();
    assert_eq!(
        cache.entries.lock().unwrap().len(),
        1,
        "completion restores retention to one oversized selected source"
    );
    cache
        .read("small".into(), None, now, || {
            Ok(json!({"tasks":[{"state":"closed","revision":73}]}))
        })
        .unwrap();
    assert_eq!(
        cache.entries.lock().unwrap().len(),
        1,
        "a later selected source evicts the idle oversized projection"
    );
}
// A4: concurrent selected metadata/history/native pages share a full folded source, never a last-page fold.
#[test]
fn selected_read_coalesces_and_revalidates_revision_ttl_and_failures() {
    let cache = std::sync::Arc::new(Selected::default());
    let now = std::time::Instant::now();
    let value = json!({"tasks":[{"revision":71,"state":"closed","workers":[{"attempt":7,"session":"codex:invented remote"}],"history":[{"text":"x".repeat(9*1024*1024)}]}]});
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let c = cache.clone();
    let v = value.clone();
    let first = std::thread::spawn(move || {
        c.read("board/task".into(), None, now, || {
            entered.send(()).unwrap();
            wait.recv().unwrap();
            Ok(v)
        })
    });
    ready.recv().unwrap();
    let c = cache.clone();
    let second = std::thread::spawn(move || {
        c.read("board/task".into(), Some(71), now, || {
            panic!("duplicate concurrent board read")
        })
    });
    release.send(()).unwrap();
    assert_eq!(first.join().unwrap().unwrap(), value);
    assert_eq!(second.join().unwrap().unwrap(), value);
    assert_eq!(
        cache
            .read("board/task".into(), Some(71), now, || panic!(
                "warm paging revalidated HTTP"
            ))
            .unwrap(),
        value
    );
    let changed = json!({"tasks":[{"revision":72,"state":"closed"}]});
    assert_eq!(
        cache
            .read("board/task".into(), Some(72), now, || Ok(changed.clone()))
            .unwrap(),
        changed
    );
    assert!(
        cache
            .read(
                "board/task".into(),
                None,
                now + std::time::Duration::from_secs(61),
                || Err("invented unavailable".into())
            )
            .is_err()
    );
    assert!(
        cache
            .read(
                "board/task".into(),
                None,
                now + std::time::Duration::from_secs(62),
                || panic!("failure was not coalesced")
            )
            .is_err()
    );
    assert_eq!(
        cache
            .read(
                "board/task".into(),
                None,
                now + std::time::Duration::from_secs(64),
                || Ok(value.clone())
            )
            .unwrap(),
        value
    );
    assert_eq!(
        worker(&value, Some(7)).unwrap(),
        ("codex".into(), "invented".into(), "remote".into(), 7)
    );
    assert!(worker(&value, Some(8)).is_err());
    let open = json!({"tasks":[{"revision":71,"state":"open"}]});
    assert_eq!(
        cache
            .read("open".into(), None, now, || Ok(open.clone()))
            .unwrap(),
        open
    );
    assert_eq!(
        cache
            .read("open".into(), None, now, || Ok(value.clone()))
            .unwrap(),
        value,
        "an open fallback must not hide a newly closed exact source"
    );
    for n in 0..20 {
        cache
            .read(n.to_string(), None, now, || Ok(changed.clone()))
            .unwrap();
    }
    assert_eq!(cache.entries.lock().unwrap().len(), 8);
}
