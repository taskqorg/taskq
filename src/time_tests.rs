use super::*;
use serde_json::Value;

fn forms() -> Value {
    serde_json::from_str(include_str!("../tests/fixtures/time-forms.json")).unwrap()
}

fn rows(key: &str) -> Vec<(i64, String)> {
    forms()[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r[0].as_i64().unwrap(), r[1].as_str().unwrap().to_string()))
        .collect()
}

// Chain: indicators.md Time -> one Rust form. Risk: a boundary rounds the wrong way, so a countdown promises a moment later than it arrives.
#[test]
fn countdown_boundaries_round_up() {
    for (secs, want) in rows("left") {
        assert_eq!(left(secs), want, "{secs} s left");
    }
}

// Chain: indicators.md Time -> one Rust form. Risk: an age reads younger or older than its largest whole unit, or a future moment reads as a negative age.
#[test]
fn age_boundaries_round_down() {
    for (secs, want) in rows("age") {
        assert_eq!(age(secs), want, "{secs} s old");
    }
}

// Chain: indicators.md Time -> exact forms. Risk: a public line carries a viewer's clock, or a viewer reads the wrong weekday or year.
#[test]
fn exact_forms_follow_the_fixture() {
    for (t, want) in rows("utc") {
        assert_eq!(utc(t), want);
    }
    for r in forms()["local"].as_array().unwrap() {
        assert_eq!(
            local_at(
                r["t"].as_i64().unwrap(),
                r["now"].as_i64().unwrap(),
                r["offset"].as_i64().unwrap()
            ),
            r["want"],
            "{r}"
        );
    }
}

// Chain: indicators.md Time -> machine form. Risk: a stored or sent time stops reading back to the same instant.
#[test]
fn machine_form_reads_back() {
    for t in [
        0, 1, 951782400, 1793888400, 1798761599, 1798761600, 4102444800,
    ] {
        assert_eq!(iso(&stamp(t)), Some(t), "{}", stamp(t));
    }
    assert_eq!(stamp(1793888400), "2026-11-05T14:20:00Z");
    assert_eq!(http_date("Thu, 05 Nov 2026 14:20:00 GMT"), Some(1793888400));
    assert_eq!(iso("not a time"), None);
}

#[test]
fn an_unknown_time_never_reads_as_a_time() {
    assert_eq!(UNKNOWN, forms()["unknown"]);
}
