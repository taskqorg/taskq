//! Every HTTP call of taskq: one curl child per request. A token travels in the stdin config, never in an argument.

use std::process::Output;
use std::time::Duration;

use coreq::process::Failure;

/// One curl child fed `config` on stdin. A curl exit that is not 0 still returns its output.
pub(crate) fn with_config(
    operation: &'static str,
    args: &[&str],
    config: &str,
    deadline: Duration,
) -> Result<Output, Failure> {
    settled(coreq::process::exchange(
        operation,
        crate::platform::command("curl").args(args),
        config.as_bytes(),
        deadline,
    ))
}

/// One curl child with no stdin, for a public URL.
pub(crate) fn with_args(
    operation: &'static str,
    args: &[&str],
    deadline: Duration,
) -> Result<Output, Failure> {
    settled(coreq::process::output(
        operation,
        crate::platform::command("curl").args(args),
        deadline,
    ))
}

fn settled(out: Result<Output, Failure>) -> Result<Output, Failure> {
    match out {
        Ok(out) | Err(Failure::Exit(out)) => Ok(out),
        Err(e) => Err(e),
    }
}

/// The body and HTTP status of a reply written with `--write-out "\n%{http_code}"`. No status: no reply came.
pub(crate) fn status_tail(out: &Output) -> (String, Option<u16>) {
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
    (
        body.to_string(),
        code.trim().parse().ok().filter(|code| *code != 0),
    )
}

/// A value quoted for a curl config file.
pub(crate) fn quote(s: &str) -> String {
    let mut q = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => q.push_str("\\\""),
            '\\' => q.push_str("\\\\"),
            '\n' => q.push_str("\\n"),
            '\r' => q.push_str("\\r"),
            '\t' => q.push_str("\\t"),
            c => q.push(c),
        }
    }
    q.push('"');
    q
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
