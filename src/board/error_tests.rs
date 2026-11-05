use super::*;

#[test]
fn a_reply_status_decides_the_reason_and_its_class() {
    for (status, text, reason, code, class) in [
        (
            401,
            "GET /x: HTTP 401 Bad credentials",
            Reason::Unauthorized,
            "unauthorized",
            "http4xx",
        ),
        (
            403,
            "GET /x: HTTP 403 Resource not accessible",
            Reason::Forbidden,
            "forbidden",
            "http4xx",
        ),
        (
            404,
            "GET /x: HTTP 404 Not Found",
            Reason::NotFound,
            "not-found",
            "http4xx",
        ),
        (
            409,
            "PUT /x: HTTP 409 Conflict",
            Reason::Refused,
            "refused",
            "http4xx",
        ),
        (
            422,
            "POST /x: HTTP 422 Validation Failed",
            Reason::Refused,
            "refused",
            "http4xx",
        ),
        (
            502,
            "GET /x: HTTP 502 Bad Gateway",
            Reason::Server,
            "server",
            "http5xx",
        ),
    ] {
        let e = Error::http(status, text);
        assert_eq!(
            (
                e.reason(),
                e.reason().code(),
                e.class(),
                e.status(),
                e.text()
            ),
            (reason, code, class, Some(status), text)
        );
    }
}

// A reply body that names another status never decides the reason: the service once read `HTTP 404` from the body of a 503.
#[test]
fn only_the_status_of_a_reply_counts_never_a_status_named_in_its_body() {
    let e = Error::http(503, "GET /x: HTTP 503 private response HTTP 404");
    assert_eq!((e.reason(), e.status()), (Reason::Server, Some(503)));
}

#[test]
fn the_board_text_names_a_rate_limit_and_a_plan_gap() {
    assert_eq!(
        Error::http(403, "GET /x: HTTP 403 API Rate Limit exceeded").reason(),
        Reason::RateLimited
    );
    assert_eq!(
        Error::http(
            403,
            "GET /x: HTTP 403 rate limit: board requests paused until 2026"
        )
        .reason(),
        Reason::RateLimited
    );
    assert_eq!(
        Error::http(403, "GET /rules: HTTP 403 Upgrade to GitHub Pro").reason(),
        Reason::PlanGap
    );
}

#[test]
fn text_without_a_reply_stays_other_and_reads_back_whole() {
    let e = Error::from(format!("bad state {}", 1));
    assert_eq!(
        (e.reason(), e.status(), e.to_string()),
        (Reason::Other, None, "bad state 1".to_string())
    );
    assert_eq!(String::from(Error::from("plain")), "plain");
    assert_eq!(
        Error::new(Reason::Timeout, "curl: timed out").class(),
        "timeout"
    );
}

// Chain: A5 reason code -> status --json schema. Risk: a reason reaches status under a code the released schema refuses.
#[test]
fn every_reason_code_is_in_the_status_schema() {
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/status-snapshot.schema.json"
    ))
    .unwrap();
    let allowed = schema["properties"]["reason_code"]["enum"]
        .as_array()
        .unwrap();
    let all = [
        Reason::Unauthorized,
        Reason::Forbidden,
        Reason::NotFound,
        Reason::Refused,
        Reason::RateLimited,
        Reason::PlanGap,
        Reason::MergeConflict,
        Reason::AlreadyQueued,
        Reason::Server,
        Reason::Timeout,
        Reason::Network,
        Reason::Invalid,
        Reason::Held,
        Reason::OutsideApi,
        Reason::Other,
    ];
    let codes: Vec<serde_json::Value> = all
        .iter()
        .map(|r| serde_json::json!(r.code()))
        .chain([serde_json::Value::Null])
        .collect();
    assert_eq!(allowed, &codes);
}
