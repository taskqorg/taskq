use super::*;

fn reply(stdout: &str) -> Output {
    let status = crate::platform::command(if cfg!(windows) { "cmd" } else { "true" }).args(if cfg!(windows) { &["/C", "exit 0"][..] } else { &[][..] }).status().unwrap();
    Output { status, stdout: stdout.as_bytes().to_vec(), stderr: vec![] }
}

// Risk: a body with newlines loses its tail, or a dead call reads as an HTTP status.
#[test]
fn the_status_tail_splits_at_the_last_line_and_zero_means_no_reply() {
    assert_eq!(status_tail(&reply("{\"a\":\n1}\n200")), ("{\"a\":\n1}".to_string(), Some(200)));
    assert_eq!(status_tail(&reply("\n000")), (String::new(), None));
    assert_eq!(status_tail(&reply("")), (String::new(), None));
}

// Risk: a value breaks out of its config line and adds a curl option.
#[test]
fn a_quoted_value_stays_on_its_config_line() {
    assert_eq!(quote("a\"b\\c\nd\re\tf"), r#""a\"b\\c\nd\re\tf""#);
}
