    use super::*;

    // Chain: §7 Result and close verdicts: the patch-id of a merged head (#1820) is Git's own `git patch-id --stable`.
    // Risk: a home-made hash that drops what Git keeps (context, file names, modes) closes a task on another patch.
    #[test]
    fn patch_id_is_gits_stable_patch_id() {
        let hex = |id: [u8; 20]| id.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let modified = json!({"filename": "src/a.rs", "status": "modified", "patch": "@@ -1,2 +1,2 @@
 fn a() {
-old();
+new();"});
        let added = json!({"filename": "b.txt", "status": "added", "patch": "@@ -0,0 +1 @@
+hi"});
        // expected: `git patch-id --stable` over the same diffs as `git diff` prints them
        assert_eq!(patch_id(&json!({"files": [modified]})).map(hex).as_deref(), Some("ee8eda765c6017d8c75984d367ee840dff07151a"));
        assert_eq!(patch_id(&json!({"files": [added, modified]})).map(hex).as_deref(), Some("91a982717c3ffcb3a0c0b1404c10f5ecfada4861"));
        assert_eq!(patch_id(&json!({"files": [{"filename": "x.png", "status": "added"}]})), None, "an omitted patch is unknown");
        assert_eq!(patch_id(&json!({"files": []})), None);
    }

    // Chain: § 2 visual evidence on GitHub (#1782). Risk: the load check or the app copy reads a URL outside this
    // repository's commit-pinned evidence, or a relative link passes as an image.
    #[test]
    fn github_evidence_urls_are_absolute_and_commit_pinned() {
        let (root, base) = ("https://api.github.com", "https://api.github.com/repos/o/r");
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let url = raw_url(root, base, sha, "1782/abc-front.png");
        assert_eq!(url, format!("https://github.com/o/r/raw/{sha}/1782/abc-front.png"));
        assert_eq!(raw_path(root, base, &url), Some((sha, "1782/abc-front.png")));
        assert_eq!(raw_path(root, base, "https://github.com/o/r/raw/taskq-evidence/1782/a.png"), None, "branch, not commit");
        assert_eq!(raw_path(root, base, &format!("https://github.com/o/x/raw/{sha}/a.png")), None, "other repository");
        assert_eq!(raw_path(root, base, &format!("https://github.com/o/r/raw/{sha}/../a.png")), None);
        assert_eq!(raw_path(root, base, "/uploads/x/a.png"), None, "relative link");
        let ghe = ("https://ghe.example/api/v3", "https://ghe.example/api/v3/repos/o/r");
        assert_eq!(raw_url(ghe.0, ghe.1, sha, "1/a.png"), format!("https://ghe.example/o/r/raw/{sha}/1/a.png"));
    }

    // Chain: § 2 visual evidence videos on GitHub (#1844). Defect: GitHub raw serves an MP4 as a download, so csgo#415's
    // videos never played from the task. Risk: a jsDelivr URL of another repository, a branch or Enterprise is load-checked.
    #[test]
    fn github_public_videos_link_the_commit_pinned_file_through_jsdelivr() {
        let (root, base) = ("https://api.github.com", "https://api.github.com/repos/o/r");
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let url = cdn_url(base, sha, "1844/abc-run.mp4");
        assert_eq!(url, format!("https://cdn.jsdelivr.net/gh/o/r@{sha}/1844/abc-run.mp4"));
        assert_eq!(raw_path(root, base, &url), Some((sha, "1844/abc-run.mp4")));
        assert_eq!(raw_path(root, base, "https://cdn.jsdelivr.net/gh/o/r@taskq-evidence/1844/a.mp4"), None, "branch, not commit");
        assert_eq!(raw_path(root, base, &format!("https://cdn.jsdelivr.net/gh/o/rx@{sha}/a.mp4")), None, "other repository");
        let ghe = ("https://ghe.example/api/v3", "https://ghe.example/api/v3/repos/o/r");
        assert_eq!(raw_path(ghe.0, ghe.1, &cdn_url(ghe.1, sha, "1/a.mp4")), None, "jsDelivr cannot reach Enterprise");
    }
