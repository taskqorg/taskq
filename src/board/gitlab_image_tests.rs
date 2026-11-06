use super::*;

// Chain: § 9 GitLab token lookup -> CLI error (#1894). Defect: with no `<install>/gitlab-token` and no `glab`, every
// board read failed with `board token lookup unavailable: process spawn: ...`, naming neither. Risk: the error loses
// the file, `glab` or the Next line, or rewrites another lookup error.
#[test]
fn missing_file_and_glab_name_both_with_next() {
    let file = std::path::Path::new("/invented/install/gitlab-token");
    let e = missing(
        file,
        "gitlab.example",
        "board token lookup unavailable: process spawn: No such file or directory (os error 2)",
    );
    let (error, next) = e.split_once('\n').unwrap();
    assert_eq!(
        error,
        format!(
            "no GitLab token: no {} and `glab` cannot run (process spawn: No such file or directory (os error 2))",
            file.display()
        )
    );
    assert_eq!(
        next,
        "Next: write a GitLab access token for gitlab.example to that file, or install glab and log in to gitlab.example"
    );
    assert_eq!(
        missing(file, "h", "no board token: set X_TOKEN or write f"),
        "no board token: set X_TOKEN or write f"
    );
    // End to end only where the host has no runnable `glab` and no env token.
    let glab = std::process::Command::new("glab")
        .arg("--version")
        .output()
        .is_ok();
    if glab || std::env::var_os("GITLAB_TOKEN").is_some() {
        return;
    }
    let install = std::env::temp_dir().join(format!("taskq-gitlab-missing-{}", std::process::id()));
    std::fs::create_dir_all(&install).unwrap();
    let tool = coreq::Tool {
        install: install.clone(),
        ..coreq::Tool::new("taskqgitlabmissingtest", "TaskQ", "fixture/unused", "1.2.3")
    };
    let e = token(&tool, "gitlab.example").unwrap_err().to_string();
    assert!(
        e.starts_with(&format!(
            "no GitLab token: no {} and `glab` cannot run (",
            install.join("gitlab-token").display()
        )),
        "{e}"
    );
    assert!(e.ends_with("\nNext: write a GitLab access token for gitlab.example to that file, or install glab and log in to gitlab.example"), "{e}");
    std::fs::remove_dir_all(install).unwrap();
}

// Chain: § 2 visual evidence on GitLab (#1782). Defect: csgo#378 embedded the upload API's relative url (and
// GitLab 17.2 full_path), which issue notes rewrite into broken repository paths. Risk: a relative or foreign URL is
// written or load-checked as if it were this project's upload.
#[test]
fn gitlab_uploads_embed_absolute_urls_and_only_own_uploads_load() {
    let root = "https://gitlab.example/api/v4";
    let reply = json!({"id": 214, "url": "/uploads/cef859da/front.png", "full_path": "/alex/csgo/uploads/cef859da/front.png", "markdown": "![front](/uploads/cef859da/front.png)"});
    let url = upload_url(root, 39, &reply).unwrap();
    assert_eq!(
        url,
        "https://gitlab.example/-/project/39/uploads/cef859da/front.png#upload=214"
    );
    assert_eq!(upload_id(root, 39, &url), Some(214));
    assert_eq!(upload_id(root, 40, &url), None, "another project's upload");
    assert_eq!(
        upload_id(root, 39, "/uploads/cef859da/front.png"),
        None,
        "relative link"
    );
    assert_eq!(
        upload_id(
            root,
            39,
            "https://gitlab.example/alex/csgo/uploads/cef859da/front.png"
        ),
        None,
        "full_path link"
    );
    assert!(
        upload_url(root, 39, &json!({"url": "/uploads/x/a.png"})).is_err(),
        "no id: no load check possible"
    );
    assert!(
        upload_url(
            root,
            39,
            &json!({"id": 1, "full_path": "/alex/csgo/uploads/x/a.png"})
        )
        .is_err()
    );
}

// Chain: § 2 visual evidence videos on GitLab (#1844). Defect: csgo#415 hand-posted host-less video links that GitLab
// rewrote into broken repository paths. Risk: a video URL keeps a fragment that stops inline play, or a foreign or
// relative URL is load-checked as this project's upload.
#[test]
fn gitlab_videos_embed_absolute_urls_ending_in_their_extension() {
    let root = "https://gitlab.example/api/v4";
    let url = upload_url(
        root,
        39,
        &json!({"id": 215, "url": "/uploads/cef859da/abcd1234-run.mp4"}),
    )
    .unwrap();
    assert_eq!(
        url,
        "https://gitlab.example/-/project/39/uploads/cef859da/abcd1234-run.mp4"
    );
    assert_eq!(video_name(root, 39, &url), Some("abcd1234-run.mp4"));
    assert_eq!(
        upload_url(
            root,
            39,
            &json!({"id": 216, "url": "/uploads/ab/clip.webm"})
        )
        .unwrap(),
        "https://gitlab.example/-/project/39/uploads/ab/clip.webm"
    );
    assert_eq!(video_name(root, 40, &url), None, "another project's upload");
    assert_eq!(
        video_name(root, 39, "/-/project/39/uploads/cef859da/abcd1234-run.mp4"),
        None,
        "host-less link"
    );
    assert_eq!(
        video_name(
            root,
            39,
            "https://gitlab.example/-/project/39/uploads/ab/front.png"
        ),
        None,
        "an image keeps its id"
    );
}
