//! Disk guard (§ 3): free space on the volumes of a project and its worktrees, read natively. It only holds new
//! launches; nothing here or in its callers deletes a file. Texts stay free of paths: they reach public heartbeats.

use std::path::Path;

/// The fewest free bytes that still admit a new worker or reviewer: exactly this much admits.
pub const MIN_FREE: u64 = 20_000_000_000;

/// One reading: free bytes on the fullest volume, or unknown when any volume did not read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Disk {
    Free(u64),
    Unknown,
}

/// Whole tenths of a GB, rounded down so a value under the threshold never prints as the threshold.
fn gb(bytes: u64) -> String {
    let tenths = bytes / 100_000_000;
    format!("{}.{} GB", tenths / 10, tenths % 10)
}

impl Disk {
    /// This host's reading for `root`: the project folder and `.worktrees`, which may be a link to another volume.
    /// An unset `.worktrees` is not a volume; a refused existence check reads, and fails, like any other path.
    pub fn read(root: &Path) -> Disk {
        let worktrees = root.join(".worktrees");
        let mut paths = vec![root];
        if worktrees.try_exists().unwrap_or(true) {
            paths.push(&worktrees);
        }
        Self::lowest(&paths, crate::platform::disk_free)
    }

    /// The fullest of `paths` under `read`; one unread path makes the whole reading unknown.
    pub fn lowest(paths: &[&Path], read: impl Fn(&Path) -> Option<u64>) -> Disk {
        paths
            .iter()
            .map(|p| read(p))
            .collect::<Option<Vec<_>>>()
            .and_then(|free| free.into_iter().min())
            .map_or(Disk::Unknown, Disk::Free)
    }

    /// `true` when this reading admits no new attempt (R12: unknown never admits).
    pub fn holds(self) -> bool {
        !matches!(self, Disk::Free(n) if n >= MIN_FREE)
    }

    /// `status.json` gate state of a holding reading.
    pub fn gate(self) -> &'static str {
        match self {
            Disk::Free(_) => "held",
            Disk::Unknown => "unknown",
        }
    }

    /// Why this reading holds: states the reading, then the rule.
    pub fn reason(self) -> String {
        match self {
            Disk::Free(n) => format!("{} free, under 20 GB: no new attempt starts", gb(n)),
            Disk::Unknown => "free space unreadable: no new attempt starts".into(),
        }
    }

    /// The one next action for a holding reading.
    pub fn next(self) -> &'static str {
        match self {
            Disk::Free(_) => {
                "free space on the project volume; taskq deletes nothing and admits again at 20 GB"
            }
            Disk::Unknown => "let the service user read the project folder and its worktrees",
        }
    }

    /// A readable free-space cell; `None` when unknown.
    pub fn free_text(self) -> Option<String> {
        match self {
            Disk::Free(n) => Some(format!("{} free", gb(n))),
            Disk::Unknown => None,
        }
    }

    /// The value of a heartbeat `disk` line, exact so a reader decides the threshold itself.
    pub fn line(self) -> String {
        match self {
            Disk::Free(n) => format!("disk {n}"),
            Disk::Unknown => "disk unknown".into(),
        }
    }

    /// Reads the text after `disk ` of a heartbeat line; anything else is no reading.
    pub fn parse(text: &str) -> Option<Disk> {
        match text {
            "unknown" => Some(Disk::Unknown),
            n => n.parse().ok().map(Disk::Free),
        }
    }
}

#[cfg(test)]
#[path = "disk_tests.rs"]
mod tests;
