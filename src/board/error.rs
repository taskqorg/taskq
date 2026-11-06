//! Why a board call failed. The reason is decided once, where the failure is met: callers branch on it and on the HTTP
//! status, and the text only reaches people and logs.

/// The cause of a failed board call; [`Reason::code`] is the wire form `status` shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// HTTP 401.
    Unauthorized,
    /// HTTP 403 without a rate limit.
    Forbidden,
    /// HTTP 404.
    NotFound,
    /// The board refused the request: any other HTTP 4xx, or a GraphQL `UNPROCESSABLE`.
    Refused,
    /// The board's rate limit, or the budget pause that follows it (§ 4).
    RateLimited,
    /// The repository plan lacks the API feature.
    PlanGap,
    /// The board refuses to enqueue a head that conflicts with main.
    MergeConflict,
    /// The head is in the merge queue already.
    AlreadyQueued,
    /// HTTP 5xx, or a status outside the HTTP error ranges.
    Server,
    /// The request reached its deadline.
    Timeout,
    /// No HTTP reply came.
    Network,
    /// The reply did not have the expected shape.
    Invalid,
    /// taskq held the request: the board budget is unknown.
    Held,
    /// taskq refused to send the token outside the board API.
    OutsideApi,
    /// Any other failure; the text says what.
    Other,
}

impl Reason {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not-found",
            Self::Refused => "refused",
            Self::RateLimited => "rate-limited",
            Self::PlanGap => "plan-gap",
            Self::MergeConflict => "merge-conflict",
            Self::AlreadyQueued => "already-queued",
            Self::Server => "server",
            Self::Timeout => "timeout",
            Self::Network => "network",
            Self::Invalid => "invalid",
            Self::Held => "held",
            Self::OutsideApi => "outside-api",
            Self::Other => "other",
        }
    }
}

/// A failed board call: its reason, the HTTP status when a reply came, and the text for people and logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    reason: Reason,
    status: Option<u16>,
    text: String,
}

impl Error {
    pub fn new(reason: Reason, text: impl Into<String>) -> Self { Self { reason, status: None, text: text.into() } }

    /// An HTTP error reply. `text` holds the reply body, the one place a board says why beyond its status.
    pub fn http(status: u16, text: impl Into<String>) -> Self {
        let text = text.into();
        let lower = text.to_ascii_lowercase();
        let reason = match status {
            _ if lower.contains("rate limit") => Reason::RateLimited,
            403 if text.contains("Upgrade to GitHub Pro") => Reason::PlanGap,
            401 => Reason::Unauthorized,
            403 => Reason::Forbidden,
            404 => Reason::NotFound,
            400..=499 => Reason::Refused,
            _ => Reason::Server,
        };
        Self { reason, status: Some(status), text }
    }

    /// A curl child that never gave a reply.
    pub fn curl(failure: coreq::process::Failure) -> Self {
        let reason = if matches!(failure, coreq::process::Failure::Timeout) { Reason::Timeout } else { Reason::Network };
        Self::new(reason, format!("curl: {failure}"))
    }

    /// A curl exit that is not 0; exit 28 is curl's own operation timeout.
    pub fn curl_exit(code: Option<i32>, text: impl Into<String>) -> Self {
        Self::new(if code == Some(28) { Reason::Timeout } else { Reason::Network }, text)
    }

    /// The same failure with its text changed, for a caller that names what it was doing.
    pub fn map_text(mut self, f: impl FnOnce(String) -> String) -> Self {
        self.text = f(self.text);
        self
    }

    pub fn reason(&self) -> Reason { self.reason }
    pub fn status(&self) -> Option<u16> { self.status }
    pub fn text(&self) -> &str { &self.text }
    /// The provider's file limit refused the call: HTTP 413, or a 4xx body that says so.
    pub fn too_large(&self) -> bool {
        matches!(self.status, Some(413)) || (matches!(self.status, Some(400..=499)) && self.text.to_ascii_lowercase().contains("too large"))
    }

    /// The telemetry class of this failure.
    pub fn class(&self) -> &'static str {
        match (self.reason, self.status) {
            (_, Some(400..=499)) => "http4xx",
            (_, Some(500..=599)) => "http5xx",
            (Reason::Timeout, _) => "timeout",
            (Reason::Invalid, _) => "invalid",
            (Reason::OutsideApi, _) => "permission",
            _ => "other",
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.text) }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(text: String) -> Self { Self::new(Reason::Other, text) }
}

impl From<&str> for Error {
    fn from(text: &str) -> Self { Self::new(Reason::Other, text) }
}

impl From<Error> for String {
    fn from(e: Error) -> Self { e.text }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
