use super::*;
use crate::core::entry;
fn log() -> Vec<Entry> {
    vec![
        Entry {
            id: 10,
            edited: false,
            body: entry("spawn", &["worker", "codex:x", "invented"], None, ""),
        },
        Entry {
            id: 11,
            edited: false,
            body: entry(
                "result",
                &[&"a".repeat(40)],
                Some(10),
                "Changed: invented\nOpen: none",
            ),
        },
        Entry {
            id: 12,
            edited: false,
            body: entry("publish", &[], None, "Changed: invented"),
        },
    ]
}
fn proof() -> Publication {
    Publication {
        repository: "invented/repo".into(),
        pr: 3,
        base: Some("b".repeat(40)),
        head: "a".repeat(40),
        integration: Some("c".repeat(40)),
        merged: Some("c".repeat(40)),
        parent: Some("b".repeat(40)),
        ci_kind: Some("merge_group".into()),
        ci_sha: Some("c".repeat(40)),
        ci_run_id: Some(123),
        ci_run_attempt: Some(2),
    }
}
// Coordination notes: a historical publication gap must not erase the proven accepted result identity.
#[test]
fn accepted_identity_survives_unknown_publication() {
    let log = log();
    let v = source(7, "https://github.com/invented/repo/issues/7", &log);
    assert_eq!(v["state"], "unknown");
    assert_eq!(v["schema"], 1);
    assert_eq!(v["accepted_result_state"], "proven");
    assert_eq!(v["result_entry"], 11);
    assert_eq!(v["attempt"], 10);
    assert_eq!(v["result_sha"], "a".repeat(40));
    assert!(v["evidence"]["frozen"].is_null());
    assert!(v["evidence"]["landed"].is_null());
    let mut rebased = log.clone();
    rebased.push(Entry {
        id: 13,
        edited: false,
        body: entry("rebased", &[&"d".repeat(40)], None, ""),
    });
    let v = source(7, "https://github.com/invented/repo/issues/7", &rebased);
    assert_eq!(v["accepted_result_state"], "proven");
    assert_eq!(v["state"], "unknown");
    assert_eq!(v["result_sha"], "a".repeat(40));
    assert_eq!(v["current_result_sha"], "d".repeat(40));
}
// A2/A3/A6: false readiness can become unsafe build authority in a later consumer.
#[test]
fn advisory_source_fences_inputs_and_never_infers_from_close_time() {
    let url = "https://github.com/invented/repo/issues/7";
    for case in [
        "ok",
        "attempt",
        "edited",
        "result",
        "head",
        "ci",
        "base",
        "parent",
        "rebase",
        "close-only",
        "private",
        "repository",
        "pr",
        "wrong-result-attempt",
        "replacement",
        "bad-run-id",
        "bad-run-attempt",
    ] {
        let mut log = log();
        let mut landed = proof();
        let mut frozen = landed.clone();
        frozen.integration = None;
        frozen.merged = None;
        frozen.parent = None;
        frozen.ci_kind = None;
        frozen.ci_sha = None;
        frozen.ci_run_id = None;
        frozen.ci_run_attempt = None;
        match case {
            "bad-run-id" => landed.ci_run_id = Some(0),
            "bad-run-attempt" => landed.ci_run_attempt = Some(0),
            "head" => landed.head = "d".repeat(40),
            "ci" => landed.ci_sha = Some("d".repeat(40)),
            "base" => frozen.base = None,
            "repository" => landed.repository = "invented/other".into(),
            "pr" => landed.pr = 4,
            "parent" => landed.parent = Some("d".repeat(40)),
            "private" => {
                landed.ci_kind = Some("private_exact_sha".into());
                landed.ci_sha = Some("a".repeat(40));
            }
            _ => {}
        }
        let text = receipt(7, url, &log, vec![frozen]);
        log.push(Entry {
            id: 13,
            edited: false,
            body: entry("enqueued", &["invented-queue"], None, &text),
        });
        let text = if case == "close-only" {
            String::new()
        } else {
            receipt(7, url, &log, vec![landed])
        };
        log.push(Entry {
            id: 14,
            edited: false,
            body: entry("close", &[], None, &text),
        });
        let plain = log
            .iter()
            .map(|e| Entry {
                body: e
                    .body
                    .lines()
                    .filter(|l| !l.starts_with("advisory "))
                    .collect::<Vec<_>>()
                    .join("\n"),
                ..e.clone()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            core::fold(7, &log, false),
            core::fold(7, &plain, false),
            "advisory cannot change fold"
        );
        match case {
            "attempt" => log[0].edited = true,
            "edited" => log[1].edited = true,
            "result" => {
                log[1].body = entry("result", &[&"d".repeat(40)], Some(10), "Changed: replaced")
            }
            "wrong-result-attempt" => {
                log[1].body = entry("result", &[&"a".repeat(40)], Some(9), "Changed: stale")
            }
            "replacement" => {
                log.truncate(4);
                log.push(Entry {
                    id: 14,
                    edited: false,
                    body: entry("requeue", &[], None, ""),
                });
                log.push(Entry {
                    id: 15,
                    edited: false,
                    body: entry("spawn", &["worker", "codex:y", "invented"], None, ""),
                });
                log.push(Entry {
                    id: 16,
                    edited: false,
                    body: entry("result", &[&"a".repeat(40)], Some(15), "Changed: replaced"),
                });
            }
            "rebase" => {
                log.pop();
                log.push(Entry {
                    id: 14,
                    edited: false,
                    body: entry("rebased", &[&"d".repeat(40)], None, ""),
                });
            }
            _ => {}
        }
        if matches!(case, "ok" | "private") {
            let v = source(7, url, &log);
            assert_eq!(v["evidence"]["landed"]["ci_run_id"], 123);
            assert_eq!(v["evidence"]["landed"]["ci_run_attempt"], 2);
        }
        assert_eq!(
            source(7, url, &log)["state"],
            if matches!(case, "ok" | "private") {
                "source_ready"
            } else {
                "unknown"
            },
            "{case}"
        );
    }
}
