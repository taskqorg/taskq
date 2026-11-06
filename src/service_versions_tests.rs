use super::*;
// A4/A6: installed/selected health and fresh board writes cannot upgrade an old running observation.
#[test]
fn advisory_binary_requires_artifact_native_identity_and_both_clocks() {
    let service = "abcdef012345";
    let e = BinaryEvidence {
        schema: 1,
        service: service.into(),
        observed: 1000,
        revision: Some("a".repeat(40)),
        digest: Some("b".repeat(64)),
        pid: Some(123),
        started: Some("invented-start".into()),
        generation: Some("invented-generation".into()),
        native_observed: Some(1000),
        state: "binary_ready".into(),
        reason: None,
    };
    let body = format!("binary-evidence {}", serde_json::to_string(&e).unwrap());
    assert_eq!(
        read_binary(&body, service, 1000, 1000, 300, false)["state"],
        "binary_ready"
    );
    for (board, now, failed) in [
        (1000, 1301, false),
        (1301, 1301, false),
        (1000, 999, false),
        (1001, 1000, false),
        (1000, 1000, true),
    ] {
        assert_eq!(
            read_binary(&body, service, board, now, 300, failed)["state"],
            "unknown"
        );
    }
    let mut partial = serde_json::to_value(&e).unwrap();
    partial["digest"] = Value::Null;
    partial["state"] = json!("unknown");
    partial["reason"] = json!("running artifact or native generation unproved");
    let partial = read_binary(
        &format!("binary-evidence {partial}"),
        service,
        1000,
        1000,
        300,
        false,
    );
    assert_eq!(partial["state"], "unknown");
    assert_eq!(partial["revision"], "a".repeat(40));
    assert!(partial["digest"].is_null());
    for key in [
        "revision",
        "digest",
        "pid",
        "started",
        "generation",
        "native_observed",
    ] {
        let mut bad = serde_json::to_value(&e).unwrap();
        bad[key] = Value::Null;
        assert_eq!(
            read_binary(
                &format!("binary-evidence {bad}"),
                service,
                1000,
                1000,
                300,
                false
            )["state"],
            "unknown",
            "{key}"
        );
    }
    assert_eq!(
        read_binary(&body, "000000000000", 1000, 1000, 300, false)["state"],
        "unknown"
    );
    assert_eq!(
        read_binary(&format!("{body}\n{body}"), service, 1000, 1000, 300, false)["state"],
        "unknown"
    );
    assert_eq!(
        read_binary(
            "version 9.9.9\nselected 9.9.9",
            service,
            1000,
            1000,
            300,
            false
        )["state"],
        "unknown"
    );
    let root =
        std::env::temp_dir().join(format!("taskq-advisory-binary-{}", crate::runtime::uuid()));
    fs::create_dir_all(root.join("releases/9.9.9")).unwrap();
    fs::write(root.join("current"), "9.9.9").unwrap();
    fs::write(root.join("releases/9.9.9/healthy"), "").unwrap();
    assert_eq!(
        binary(&root, service, None)["state"],
        "unknown",
        "installed markers cannot prove this running service"
    );
    fs::remove_dir_all(root).unwrap();
}
// D18: selection/health sources and clocks, unavailable sources, malformed input and stale health.
#[test]
fn projection_preserves_source_clocks_and_never_invents_health() {
    let root = std::env::temp_dir().join(format!("taskq-versions-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("releases/4.6.19")).unwrap();
    let id = "abcdef012345";
    let now = clock();
    let missing = collect_at(&root, "4.6.19", id, now, false, now);
    assert_eq!(missing["selected"]["state"], "unknown");
    assert!(missing["health"]["value"].is_null());
    fs::write(root.join("current"), "4.6.19").unwrap();
    fs::write(root.join("releases/4.6.19/healthy"), "").unwrap();
    let now = clock();
    let value = collect_at(&root, "4.6.19", id, now.saturating_sub(1), false, now);
    let body = format!("version 4.6.19\nservice {id}\ndiagnostics {value}");
    let fresh = read(&body, now as i64, now as i64, 300, false);
    assert_eq!(fresh["selected"]["value"], "4.6.19");
    assert_eq!(fresh["health"]["state"], "ok");
    assert_eq!(fresh["coreq"]["value"], env!("TASKQ_COREQ_VERSION"));
    assert!(fresh["available"]["value"].is_null());
    for (at, failed) in [(now + 301, false), (now, true)] {
        let stale = read(&body, now as i64, at as i64, 300, failed);
        assert!(stale["health"]["value"].is_null());
        assert_eq!(stale["selected"]["at"], fresh["selected"]["at"]);
    }
    assert_eq!(
        collect_at(&root, "4.6.19", id, now, false, now)["health"]["state"],
        "last_known"
    );
    let old = collect_at(&root, "4.6.19", id, now + 1, false, now + 1);
    assert_eq!(old["health"]["state"], "last_known");
    assert_eq!(old["lifecycle"]["value"], "starting");
    assert_eq!(
        collect_at(&root, "4.6.18", id, now, true, now)["lifecycle"]["value"],
        "waiting"
    );
    for body in [
        "",
        "diagnostics {}",
        &format!("{body}\ndiagnostics {{}}"),
        &body.replace(id, "bad"),
    ] {
        assert_eq!(
            read(body, now as i64, now as i64, 300, false)["state"],
            "unknown"
        );
    }
    for (key, replacement) in [("schema", json!(2)), ("service", json!("000000000000"))] {
        let mut bad = value.clone();
        bad[key] = replacement;
        assert_eq!(
            read(
                &format!("service {id}\ndiagnostics {bad}"),
                now as i64,
                now as i64,
                300,
                false
            )["state"],
            "unknown"
        );
    }
    assert_eq!(
        read(&body, now as i64, now as i64 - 1, 300, false)["running"]["state"],
        "unknown"
    );
    let mut bad = value.clone();
    bad["selected"]["reason"] = json!("private metadata");
    assert_eq!(
        read(
            &format!("service {id}\ndiagnostics {bad}"),
            now as i64,
            now as i64,
            300,
            false
        )["state"],
        "unknown"
    );
    fs::write(root.join("current"), "../../secret").unwrap();
    assert!(collect_at(&root, "4.6.19", id, now, false, now)["selected"]["value"].is_null());
    fs::remove_dir_all(root).unwrap();
}
