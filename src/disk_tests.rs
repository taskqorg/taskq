use super::*;
use std::collections::BTreeMap;

// Chain: § 3 Disk guard. Risk: full disks kill reviewers and builds (os error 112, ENOSPC) when a threshold edge or
// a second volume is misread. Protection: below holds, at and above admit, on Unix and Windows path shapes; the
// fullest volume decides; one unread volume reads unknown and holds.
#[test]
fn disk_threshold_decides_on_the_fullest_volume_of_unix_and_windows_paths() {
    let cases: [[&str; 2]; 3] = [
        ["/srv/project", "/mnt/work/.worktrees"],
        ["C:\\Users\\agent\\project", "D:\\work\\.worktrees"],
        ["\\\\share\\agents\\project", "E:\\.worktrees"],
    ];
    for [root, worktrees] in cases {
        let paths = [Path::new(root), Path::new(worktrees)];
        for (free, holds) in [
            (MIN_FREE - 1, true),
            (MIN_FREE, false),
            (MIN_FREE + 1, false),
        ] {
            // the worktree volume is the fuller one, then the project volume
            for (r, w) in [(MIN_FREE * 2, free), (free, MIN_FREE * 2)] {
                let read = BTreeMap::from([(root, r), (worktrees, w)]);
                let disk = Disk::lowest(&paths, |p| read.get(p.to_str()?).copied());
                assert_eq!(disk, Disk::Free(free), "{root}");
                assert_eq!(disk.holds(), holds, "{root} {free}");
            }
        }
        let unread = Disk::lowest(&paths, |p| (p == Path::new(root)).then_some(MIN_FREE * 2));
        assert_eq!(unread, Disk::Unknown, "{root}");
        assert!(unread.holds());
    }
}

// Risk: the real reader reports nothing or a wrong success on this OS (the CI matrix runs Unix and Windows).
// Protection: an existing folder reads a positive value; a missing one reads unknown and holds.
#[test]
fn disk_reader_reads_a_real_folder_and_a_missing_one_stays_unknown() {
    let dir = std::env::temp_dir().join(format!("taskq-disk-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(crate::platform::disk_free(&dir).is_some_and(|n| n > 0));
    // no `.worktrees` yet is no volume to read
    assert!(matches!(Disk::read(&dir), Disk::Free(n) if n > 0));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(crate::platform::disk_free(&dir), None);
    assert_eq!(Disk::read(&dir), Disk::Unknown);
}

// Risk: public text rounds a held reading up to the threshold, or a heartbeat value is misread by a reader.
#[test]
fn disk_text_never_rounds_up_and_the_heartbeat_value_round_trips() {
    assert!(Disk::Free(MIN_FREE - 1).reason().starts_with("19.9 GB free"));
    assert_eq!(Disk::Free(MIN_FREE).free_text().unwrap(), "20.0 GB free");
    for d in [Disk::Free(MIN_FREE - 1), Disk::Unknown] {
        assert_eq!(Disk::parse(d.line().strip_prefix("disk ").unwrap()), Some(d));
    }
    assert_eq!(Disk::parse("much"), None);
}

// Risk: the tray and the remote projection drop the new reason and read a held host as ready or healthy.
#[test]
fn disk_reason_survives_the_gate_line_and_the_remote_projection() {
    let held = serde_json::json!({"gate": "held", "reasons": [{"code": "disk", "text": "x"}]});
    let unread = serde_json::json!({"gate": "unknown", "reasons": [{"code": "disk", "text": "y"}]});
    assert_eq!(crate::cli::host_cmd::gate_text(&held), "Waiting: low disk");
    assert_eq!(crate::cli::host_cmd::gate_text(&unread), "Unknown: y");
    let code = |c| crate::resources::blockers(c, |_| serde_json::json!(null))[0].clone();
    assert_eq!((code(&held)["code"].as_str(), code(&held)["knowledge"].as_str()), (Some("disk"), Some("known")));
    assert_eq!(code(&unread)["knowledge"], "unknown");
}
