//! Every time this product writes or reads, in one place (specq caveman/rules/indicators.md, section Time):
//! `left` for a countdown, `age` for a past moment, `local` for one viewer, `utc` across hosts and in public text,
//! `stamp` for machines. `src/relative-time.js` renders the same forms; `tests/fixtures/time-forms.json` pins both.

const MINUTE: i64 = 60;
const HOUR: i64 = 3600;
const DAY: i64 = 86400;
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// An unknown time; never `0:00`, `now` or an empty cell.
pub const UNKNOWN: &str = "? unavailable";

/// Age of a past moment: the largest whole unit, rounded down. A negative age reads `<1m`.
pub fn age(secs: i64) -> String {
    match secs.max(0) {
        s if s < MINUTE => "<1m".into(),
        s if s < HOUR => format!("{}m", s / MINUTE),
        s if s < DAY => format!("{}h", s / HOUR),
        s => format!("{}d", s / DAY),
    }
}

/// Countdown to a future moment, rounded up so the moment never arrives later than the number says.
pub fn left(secs: i64) -> String {
    match secs {
        s if s <= 0 => "now".into(),
        s if s < 5 * HOUR => {
            let minutes = (s + MINUTE - 1) / MINUTE;
            format!("{}:{:02}", minutes / 60, minutes % 60)
        }
        s if s < DAY => format!("{}h", (s + HOUR - 1) / HOUR),
        s => format!("{}d", (s + DAY - 1) / DAY),
    }
}

/// Exact time across hosts, in board comments, logs and public text: `2026-11-05 14:30 UTC`.
pub fn utc(t: i64) -> String {
    let c = Civil::of(t);
    format!("{:04}-{:02}-{:02} {:02}:{:02} UTC", c.year, c.month, c.day, c.hour, c.minute)
}

/// Exact time for one viewer on this host's clock: `Thu Nov 05, 14:30`, with the year when it is not the current one.
pub fn local(t: i64, now: i64) -> String {
    local_at(t, now, crate::platform::utc_offset(t))
}

fn local_at(t: i64, now: i64, offset: i64) -> String {
    let (c, current) = (Civil::of(t + offset), Civil::of(now + offset));
    let year = if c.year == current.year { String::new() } else { format!(" {:04}", c.year) };
    format!("{} {} {:02}{year}, {:02}:{:02}", c.weekday(), MONTHS[c.month as usize - 1], c.day, c.hour, c.minute)
}

/// For machines: RFC 3339 in UTC. Never read a display form back; `iso` reads this one.
pub fn stamp(t: i64) -> String {
    let c = Civil::of(t);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", c.year, c.month, c.day, c.hour, c.minute, c.second)
}

/// `2026-10-12T10:00:00Z`, with optional fraction or `+00:00`; UTC only, as both boards send.
pub fn iso(s: &str) -> Option<i64> {
    let n = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse::<i64>().ok());
    Some(days(n(0, 4)?, n(5, 7)?, n(8, 10)?) * DAY + n(11, 13)? * HOUR + n(14, 16)? * MINUTE + n(17, 19)?)
}

/// A machine time read from a record, written for people.
pub fn shown(s: &str) -> String {
    iso(s).map_or_else(|| UNKNOWN.to_string(), utc)
}

/// HTTP `Date`: `Mon, 12 Oct 2026 10:00:00 GMT`.
pub fn http_date(s: &str) -> Option<i64> {
    let w: Vec<&str> = s.split_whitespace().collect();
    let mon = *w.get(2)?;
    let m = MONTHS.iter().position(|x| *x == mon)? as i64 + 1;
    let t: Vec<i64> = w.get(4)?.split(':').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    Some(days(w.get(3)?.parse().ok()?, m, w.get(1)?.parse().ok()?) * DAY + t.first()? * HOUR + t.get(1)? * MINUTE + t.get(2)?)
}

/// Days since the epoch of a civil date (Hinnant).
fn days(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}

struct Civil {
    days: i64,
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
}

impl Civil {
    /// Civil from days (Hinnant).
    fn of(t: i64) -> Self {
        let (days, r) = (t.div_euclid(DAY), t.rem_euclid(DAY));
        let z = days + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
        Civil { days, year, month, day, hour: r / HOUR, minute: r % HOUR / MINUTE, second: r % MINUTE }
    }

    /// The epoch day was a Thursday.
    fn weekday(&self) -> &'static str {
        WEEKDAYS[(self.days + 4).rem_euclid(7) as usize]
    }
}

#[cfg(test)]
#[path = "time_tests.rs"]
mod tests;
