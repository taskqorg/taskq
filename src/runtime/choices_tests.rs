use super::*;
// R9/A4: permission requests only narrow existing authority; neither stale state nor Full requests grant access.
#[test]
fn requests_narrow_and_preserve_the_operator_store() {
    let root = std::env::temp_dir().join(format!("taskq-choice-{}", super::super::uuid()));
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let file = root.join(crate::policy::FILE);
    let policy = json!({"revision":7,"grants":[{"runtime":"claude","mode":"full","project":root,"by":"invented operator","at":1}]});
    fs::write(&file, policy.to_string()).unwrap();
    let mut choice = Choice {
        permission: "workspace".into(),
        grant_mode: "full".into(),
        grant_revision: Some(7),
        ..Default::default()
    };
    let result = choice.access(&root, &root, "claude", "dontAsk", &[]);
    if cfg!(target_os = "macos") {
        let access = result.unwrap();
        assert_eq!(access.mode, "workspace");
        assert!(access.profile.is_some());
        assert!(access.grant.starts_with("full (project)"));
    } else {
        assert!(result.is_err(), "unsupported containment refuses");
    }
    assert_eq!(fs::read_to_string(&file).unwrap(), policy.to_string());
    choice.grant_revision = Some(6);
    assert!(
        choice
            .access(&root, &root, "claude", "dontAsk", &[])
            .unwrap_err()
            .contains("grant changed")
    );
    choice.grant_revision = Some(7);
    choice.permission = "full".into();
    let mut narrowed = policy.clone();
    narrowed["grants"][0]["mode"] = json!("workspace");
    fs::write(&file, narrowed.to_string()).unwrap();
    assert!(
        choice
            .access(&root, &root, "claude", "dontAsk", &[])
            .unwrap_err()
            .contains("requires an existing")
    );
    fs::remove_dir_all(root).unwrap();
}
// A1/R12: a generic CLI effort flag is not a model-specific catalog.
#[test]
fn native_catalogs_never_invent_model_effort_pairs() {
    let aliases = native_models(
        true,
        "Provide an alias e.g. 'opus' or 'sonnet'; --effort low medium high xhigh max",
    )
    .unwrap();
    assert!(
        aliases["models"]
            .as_array()
            .unwrap()
            .iter()
            .all(|model| model["efforts"] == json!([{"effort":""}]))
    );
    let native = native_models(
        false,
        "gemini-4-argon-low\tGemini 4 Argon (Low)\ninvalid row",
    )
    .unwrap();
    assert_eq!(native["models"][0]["id"], "gemini-4-argon-low");
    assert_eq!(native["models"][0]["efforts"], json!([{"effort":"low"}]));
    assert!(native_models(false, "unreadable output").is_err());
}

// A1/R9: one supported exact choice replaces only model/effort, preserving permission/network requests.
#[test]
fn explicit_codex_choice_keeps_other_requests() {
    let choice = Choice {
        model: "gpt-6.1-sol".into(),
        effort: "low".into(),
        ..Default::default()
    };
    let original = [
        "-s",
        "workspace-write",
        "--model=old",
        "-c",
        "model_reasoning_effort=high",
        "-c",
        "sandbox_workspace_write.network_access=true",
    ]
    .map(String::from);
    assert_eq!(
        choice.codex(&original),
        [
            "-s",
            "workspace-write",
            "-c",
            "sandbox_workspace_write.network_access=true",
            "-m",
            "gpt-6.1-sol",
            "-c",
            "model_reasoning_effort=low"
        ]
        .map(String::from)
    );
    assert!(
        !worker_defaults("codex", &["-c".into(), "api_key=invented-secret".into()])
            .contains("invented-secret")
    );
}
