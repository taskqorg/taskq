//! Host doctor (#2048, [host service](../docs/design/host-service.md#host-doctor)): one host-wide pass, hourly, that
//! removes stale releases of `<install>/releases` and leftovers of qualification and canary runs: OS-temp folders and
//! the native Scheduled Tasks, user units and launchd jobs those runs named. It never touches a release in use, a live
//! worker's private temp, `.worktrees` or anything younger than [`STALE`]; its counts carry no paths.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Seconds between two doctor passes; the first one runs [`FIRST`] seconds after the host owner starts.
pub const EVERY: u64 = 3600;
const FIRST: u64 = 600;
/// Releases kept besides the ones in use (current, previous, running, each registered project's): the newest ones.
pub const KEEP_PREVIOUS: usize = 2;
/// A leftover whose newest entry is younger than this may belong to a run still in progress: it stays.
pub const STALE: Duration = Duration::from_secs(86_400);
/// Temp folders removed by one pass at most; a larger backlog drains over later passes.
const MAX_TEMP: usize = 2000;
/// OS-temp folders the doctor never removes: live worker private temp (Windows sandbox).
const KEEP_TEMP: [&str; 1] = ["taskq-windows"];

/// What one pass removed and kept; `freed` is the byte size of removed files.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome { pub releases: u64, pub temp: u64, pub native: u64, pub freed: u64, pub kept: u64, pub failed: u64, pub unknown: u64 }

impl Outcome {
    fn counts(&self) -> BTreeMap<String, u64> {
        [("releases", self.releases), ("temp", self.temp), ("native", self.native), ("removed", self.releases + self.temp + self.native),
            ("freed_bytes", self.freed), ("kept", self.kept), ("failed", self.failed), ("unknown", self.unknown)]
            .into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }
    /// One host log line, only when the pass removed or failed to remove something, or release ownership was unknown.
    fn line(&self) -> Option<String> {
        (self.releases + self.temp + self.native + self.failed + self.unknown > 0).then(|| {
            let mut line = format!("doctor: removed {} old releases, {} temp folders, {} qualification objects; freed {} MiB",
                self.releases, self.temp, self.native, self.freed / (1 << 20));
            if self.failed > 0 { line.push_str(&format!("; {} removals failed", self.failed)); }
            if self.unknown > 0 { line.push_str("; release ownership unknown, every release kept"); }
            line
        })
    }
}

/// The host loop's doctor schedule: one background pass at a time, so a large backlog never delays a host tick.
pub struct Doctor { began: Instant, last: Option<Instant>, running: Option<std::thread::JoinHandle<Option<String>>> }

impl Default for Doctor {
    fn default() -> Self { Doctor { began: Instant::now(), last: None, running: None } }
}

impl Doctor {
    /// Starts a due pass and returns the log line of a finished one.
    pub fn tick(&mut self, install: &Path) -> Option<String> {
        let mut line = None;
        if self.running.as_ref().is_some_and(std::thread::JoinHandle::is_finished) {
            line = self.running.take().and_then(|h| h.join().unwrap_or_else(|_| Some("doctor: pass panicked; outcome unknown".into())));
        }
        let due = self.last.map_or(self.began.elapsed().as_secs() >= FIRST, |l| l.elapsed().as_secs() >= EVERY);
        if self.running.is_none() && due {
            self.last = Some(Instant::now());
            let install = install.to_path_buf();
            self.running = std::thread::Builder::new().name("taskq-doctor".into())
                .spawn(move || pass(&install, &std::env::temp_dir(), SystemTime::now(), &Native::Host).line()).ok();
        }
        line
    }
}

/// One doctor pass, recorded as the `service.doctor` operation with its counts.
pub fn pass(install: &Path, temp: &Path, now: SystemTime, native: &Native) -> Outcome {
    crate::profile::measure_counted("service.doctor", || {
        let mut o = Outcome::default();
        releases(install, now, &mut o);
        leftovers(temp, now, &mut o);
        native.clean(now, &mut o);
        let counts = o.counts();
        (o, counts)
    })
}

/// Byte size and newest file modification time of a tree (the root's own time when it holds no file); symbolic links
/// count as files and are never followed.
fn tree(path: &Path) -> (u64, SystemTime) {
    let (mut bytes, mut newest) = (0, None::<SystemTime>);
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(meta) = fs::symlink_metadata(&p) else { continue };
        if meta.is_dir() {
            stack.extend(fs::read_dir(&p).into_iter().flatten().flatten().map(|e| e.path()));
        } else {
            bytes += meta.len();
            if let Ok(m) = meta.modified() { newest = Some(newest.map_or(m, |n| n.max(m))); }
        }
    }
    let root = || fs::symlink_metadata(path).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
    (bytes, newest.unwrap_or_else(root))
}

fn stale(newest: SystemTime, now: SystemTime) -> bool { now.duration_since(newest).is_ok_and(|age| age >= STALE) }

fn remove(path: &Path, bytes: u64, o: &mut Outcome) -> bool {
    let meta = fs::symlink_metadata(path);
    let gone = if meta.as_ref().is_ok_and(|m| m.is_dir()) { fs::remove_dir_all(path) } else { fs::remove_file(path) };
    match gone {
        Ok(()) => { o.freed += bytes; true }
        Err(_) => { o.failed += 1; false }
    }
}

/// A1: keep every release in use plus the [`KEEP_PREVIOUS`] newest others; remove the rest and staging leftovers
/// (`*.tmp`) older than [`STALE`]. Unknown ownership (registry or a project version unreadable) keeps everything.
fn releases(install: &Path, now: SystemTime, o: &mut Outcome) {
    let dir = install.join("releases");
    let Ok(entries) = fs::read_dir(&dir) else { return };
    let mut entries: Vec<(String, PathBuf, u64, SystemTime)> = entries.flatten()
        .map(|e| { let (bytes, newest) = tree(&e.path()); (e.file_name().to_string_lossy().into_owned(), e.path(), bytes, newest) }).collect();
    let Some(in_use) = crate::platform::releases_in_use(install) else {
        o.unknown += 1;
        o.kept += entries.len() as u64;
        return;
    };
    entries.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| b.0.cmp(&a.0)));
    let mut previous = 0;
    for (name, path, bytes, newest) in entries {
        if name.ends_with(".tmp") {
            if stale(newest, now) && remove(&path, bytes, o) { o.releases += 1; }
            continue;
        }
        if in_use.contains(&name) || previous < KEEP_PREVIOUS {
            if !in_use.contains(&name) { previous += 1; }
            o.kept += 1;
        } else if remove(&path, bytes, o) {
            o.releases += 1;
        }
    }
}

/// A taskq or coreq folder in the OS temp dir: test, qualification and canary scratch. Live private temp stays.
fn leftover(name: &str) -> bool {
    (name.starts_with("taskq-") || name.starts_with("coreq-")) && !KEEP_TEMP.contains(&name)
}

/// A2: remove OS-temp leftover folders whose whole tree is older than [`STALE`].
fn leftovers(temp: &Path, now: SystemTime, o: &mut Outcome) {
    let Ok(entries) = fs::read_dir(temp) else { return };
    let mut removed = 0;
    for e in entries.flatten() {
        if removed >= MAX_TEMP { break; }
        let name = e.file_name().to_string_lossy().into_owned();
        if !leftover(&name) || !fs::symlink_metadata(e.path()).is_ok_and(|m| m.is_dir()) { continue; }
        let (bytes, newest) = tree(&e.path());
        if !stale(newest, now) { continue; }
        if remove(&e.path(), bytes, o) { o.temp += 1; removed += 1; }
    }
}

/// A native object a qualification or canary run named: a `Qualification` or `Canary` marker together with the product
/// or a runtime it qualifies. The host's own unit never matches.
pub fn qualification_object(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    (n.contains("qualification") || n.contains("canary")) && ["taskq", "coreq"].into_iter().chain(crate::runtime::RUNTIMES).any(|p| n.contains(p))
}

/// One native scheduler entry: name, whether it runs now, and seconds since the epoch of its last activity (0: never).
#[derive(Debug, PartialEq)]
pub struct Entry { pub name: String, pub running: bool, pub last: u64 }

/// Entries the doctor removes: qualification objects that are not running and were last active before [`STALE`];
/// a never-active entry has no known age and stays. Linux also resets failed `taskq-*` transient units.
pub fn removable(entries: &[Entry], now: SystemTime, owned_prefix: Option<&str>) -> Vec<String> {
    let now = now.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    entries.iter().filter(|e| !e.running && e.last > 0 && now.saturating_sub(e.last) >= STALE.as_secs())
        .filter(|e| qualification_object(&e.name) || owned_prefix.is_some_and(|p| e.name.starts_with(p)))
        .map(|e| e.name.clone()).collect()
}

/// Native scheduler access; tests use [`Native::None`].
pub enum Native { #[cfg_attr(not(test), allow(dead_code))] None, Host }

impl Native {
    fn clean(&self, now: SystemTime, o: &mut Outcome) {
        if matches!(self, Native::None) { return; }
        match native_clean(now) {
            Ok((removed, failed)) => { o.native += removed; o.failed += failed; }
            Err(_) => o.failed += 1,
        }
    }
}

const NATIVE_BUDGET: Duration = Duration::from_secs(60);

/// Windows Scheduled Tasks of the root folder: state 4 is running; `LastRunTime` before 2000 means never run.
#[cfg(windows)]
fn native_clean(now: SystemTime) -> Result<(u64, u64), String> {
    const LIST: &str = "$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$r=$s.GetFolder('\\');foreach($t in $r.GetTasks(1)){$l=$t.LastRunTime;$u=if($l.Year -lt 2000){0}else{[DateTimeOffset]::new($l.ToUniversalTime()).ToUnixTimeSeconds()};\"{0}`t{1}`t{2}\" -f $t.Name,$t.State,$u}";
    let out = crate::platform::output("doctor.tasks", &mut crate::host_control::powershell(LIST), NATIVE_BUDGET)?;
    if !out.status.success() { return Err("scheduled tasks unreadable".into()); }
    let names = removable(&windows_tasks(&String::from_utf8_lossy(&out.stdout)), now, None);
    if names.is_empty() { return Ok((0, 0)); }
    let quoted: Vec<String> = names.iter().map(|n| format!("'{}'", n.replace('\'', "''"))).collect();
    let code = format!("$s=New-Object -ComObject 'Schedule.Service';$s.Connect();$r=$s.GetFolder('\\');foreach($n in @({})){{try{{$r.DeleteTask($n,0);'removed'}}catch{{'failed'}}}}", quoted.join(","));
    let out = crate::platform::output("doctor.tasks.remove", &mut crate::host_control::powershell(&code), NATIVE_BUDGET)?;
    let text = String::from_utf8_lossy(&out.stdout);
    let removed = text.lines().filter(|l| l.trim() == "removed").count() as u64;
    Ok((removed, names.len() as u64 - removed))
}

/// Parses `name<TAB>state<TAB>last-run-seconds` lines of the Scheduled Task listing.
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_tasks(text: &str) -> Vec<Entry> {
    text.lines().filter_map(|l| {
        let mut f = l.trim_end().split('\t');
        let (name, state, last) = (f.next()?, f.next()?, f.next()?);
        Some(Entry { name: name.into(), running: state != "1" && state != "3", last: last.parse().ok()? })
    }).collect()
}

/// systemd user units: failed qualification or `taskq-*` units are reset; the service unit never matches.
#[cfg(target_os = "linux")]
fn native_clean(now: SystemTime) -> Result<(u64, u64), String> {
    let out = crate::platform::output("doctor.units", crate::platform::command("systemctl").args(["--user", "list-units", "--all", "--state=failed",
        "--plain", "--no-legend", "--no-pager"]), NATIVE_BUDGET)?;
    if !out.status.success() { return Err("user units unreadable".into()); }
    let failed: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.split_whitespace().next().map(String::from))
        .filter(|u| u != "taskq.service").collect();
    if failed.is_empty() { return Ok((0, 0)); }
    let show = crate::platform::output("doctor.units.age", crate::platform::command("systemctl").args(["--user", "show", "--property=Id",
        "--property=StateChangeTimestampMonotonic", "--no-pager"]).args(&failed), NATIVE_BUDGET)?;
    let uptime = fs::read_to_string("/proc/uptime").ok().and_then(|t| t.split_whitespace().next()?.parse::<f64>().ok())
        .ok_or("uptime unknown")?;
    let names = removable(&linux_units(&String::from_utf8_lossy(&show.stdout), uptime, now), now, Some("taskq-"));
    let (mut removed, mut failures) = (0, 0);
    for unit in names.into_iter().filter(|u| u != "taskq.service") {
        match crate::platform::output("doctor.units.reset", crate::platform::command("systemctl").args(["--user", "reset-failed", &unit]), NATIVE_BUDGET) {
            Ok(o) if o.status.success() => removed += 1,
            _ => failures += 1,
        }
    }
    Ok((removed, failures))
}

/// Parses `systemctl show` blocks of `Id=` and `StateChangeTimestampMonotonic=` (microseconds since boot) into failed
/// entries whose last activity is in epoch seconds.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn linux_units(text: &str, uptime: f64, now: SystemTime) -> Vec<Entry> {
    let now = now.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let boot = now.saturating_sub(uptime as u64);
    text.split("\n\n").filter_map(|block| {
        let field = |k: &str| block.lines().find_map(|l| l.strip_prefix(k)).map(str::trim);
        let id = field("Id=")?;
        let changed: u64 = field("StateChangeTimestampMonotonic=")?.parse().ok()?;
        (changed > 0).then(|| Entry { name: id.into(), running: false, last: boot + changed / 1_000_000 })
    }).collect()
}

/// launchd user jobs: a loaded qualification job without a process is removed; launchd keeps no age, so the job's
/// last exit is taken as its end.
#[cfg(target_os = "macos")]
fn native_clean(_now: SystemTime) -> Result<(u64, u64), String> {
    let out = crate::platform::output("doctor.jobs", crate::platform::command("launchctl").arg("list"), NATIVE_BUDGET)?;
    if !out.status.success() { return Err("launchd jobs unreadable".into()); }
    let (mut removed, mut failures) = (0, 0);
    for label in mac_jobs(&String::from_utf8_lossy(&out.stdout)) {
        match crate::platform::output("doctor.jobs.remove", crate::platform::command("launchctl").args(["remove", &label]), NATIVE_BUDGET) {
            Ok(o) if o.status.success() => removed += 1,
            _ => failures += 1,
        }
    }
    Ok((removed, failures))
}

/// Labels of `launchctl list` (`PID<TAB>status<TAB>label`) naming an idle qualification job.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn mac_jobs(text: &str) -> Vec<String> {
    text.lines().filter_map(|l| {
        let mut f = l.split('\t');
        let (pid, _status, label) = (f.next()?, f.next()?, f.next()?.trim());
        (pid == "-" && qualification_object(label) && label != "com.taskq.service").then(|| label.to_string())
    }).collect()
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn native_clean(_now: SystemTime) -> Result<(u64, u64), String> { Ok((0, 0)) }

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("taskq-doctor-{tag}-{}", crate::runtime::uuid()));
        fs::create_dir_all(&d).unwrap();
        d
    }
    fn file(p: &Path, bytes: usize, at: SystemTime) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![b'x'; bytes]).unwrap();
        fs::File::options().write(true).open(p).unwrap().set_modified(at).unwrap();
    }
    fn names(dir: &Path) -> Vec<String> {
        let mut n: Vec<String> = fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        n.sort();
        n
    }

    // #2048 defect: only a release update swept old releases, so a dev-channel host kept 299 (4.3 GB). The doctor keeps
    // releases in use plus the two newest others and removes the rest, with their bytes counted.
    #[test]
    fn doctor_keeps_releases_in_use_and_two_previous() {
        let base = scratch("releases");
        let install = base.join("install");
        let now = SystemTime::now() + Duration::from_secs(2 * 86_400);
        let day = |n: u64| SystemTime::now() - Duration::from_secs(n * 86_400);
        for (i, v) in ["4.0.1", "4.0.2", "4.0.3", "4.0.4", "4.0.5", "4.0.6"].iter().enumerate() {
            file(&install.join("releases").join(v).join("taskq"), 100, day(10 - i as u64));
        }
        file(&install.join("releases/4.0.7.tmp/taskq"), 10, day(30));
        file(&install.join("releases/4.0.8.tmp/taskq"), 10, now);
        fs::write(install.join("current"), "4.0.2").unwrap();
        fs::write(install.join("previous"), "4.0.1").unwrap();
        fs::write(install.join("projects"), "").unwrap();
        let o = pass(&install, &base.join("none"), now, &Native::None);
        let on_disk = names(&install.join("releases"));
        for v in ["4.0.1", "4.0.2", "4.0.5", "4.0.6", "4.0.8.tmp"] { assert!(on_disk.contains(&v.to_string()), "{v} kept: {on_disk:?}"); }
        for v in ["4.0.3", "4.0.4", "4.0.7.tmp"] { assert!(!on_disk.contains(&v.to_string()), "{v} removed: {on_disk:?}"); }
        assert_eq!((o.releases, o.freed, o.kept, o.failed, o.unknown), (3, 210, 4, 0, 0));
        let _ = fs::remove_dir_all(&base);
    }

    // Kept protection: an unreadable registry leaves release ownership unknown, so no release is removed.
    #[test]
    fn doctor_keeps_every_release_when_ownership_is_unknown() {
        let base = scratch("unknown");
        let install = base.join("install");
        for v in ["4.0.1", "4.0.2", "4.0.3", "4.0.4"] { file(&install.join("releases").join(v).join("taskq"), 1, SystemTime::UNIX_EPOCH + Duration::from_secs(86_400)); }
        fs::write(install.join("current"), "4.0.4").unwrap();
        fs::create_dir_all(install.join("projects")).unwrap();
        let o = pass(&install, &base.join("none"), SystemTime::now(), &Native::None);
        assert_eq!(names(&install.join("releases")).len(), 4);
        assert_eq!((o.releases, o.unknown, o.kept), (0, 1, 4));
        assert!(o.line().unwrap().contains("ownership unknown"));
        let _ = fs::remove_dir_all(&base);
    }

    // #2048 defect: qualification and test scratch stayed in the OS temp dir for good. Kept protection: live worker
    // private temp, foreign folders, files and anything touched within a day stay.
    #[test]
    fn doctor_removes_stale_temp_leftovers_only() {
        let base = scratch("temp");
        let temp = base.join("temp");
        let now = SystemTime::now() + Duration::from_secs(2 * 86_400);
        let old = SystemTime::now();
        file(&temp.join("taskq-candidate-a422e70/evidence/run.log"), 40, old);
        file(&temp.join("coreq-host-installer-qualification-1/x"), 2, old);
        file(&temp.join("taskq-windows/abc/live"), 5, old);
        file(&temp.join("taskq-setup-1/old"), 3, old);
        file(&temp.join("taskq-setup-1/fresh"), 3, now);
        file(&temp.join("other-tool/x"), 1, old);
        file(&temp.join("taskq-tray.lock"), 1, old);
        let o = pass(&base.join("install"), &temp, now, &Native::None);
        assert_eq!(names(&temp), ["other-tool", "taskq-setup-1", "taskq-tray.lock", "taskq-windows"]);
        assert_eq!((o.temp, o.freed, o.failed), (2, 42, 0));
        assert_eq!(o.counts()["removed"], 2);
        assert_eq!(o.counts()["freed_bytes"], 42);
        let _ = fs::remove_dir_all(&base);
    }

    // #2048 defect: Scheduled Tasks and user units of qualification runs stayed registered. Kept protection: the host's
    // own unit, running entries, fresh entries and never-run entries stay.
    #[test]
    fn doctor_selects_idle_stale_qualification_objects() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10 * 86_400);
        let old = 8 * 86_400;
        let tasks = windows_tasks(&format!("CodexQualification-a422e70\t3\t{old}\nQualification-F90-TaskQ-Runtime\t1\t{old}\nTaskQ\t4\t{old}\n\
            Claude-Canary-run\t4\t{old}\nTaskQ-Qualification-new\t3\t{}\nTaskQ-Qualification-never\t3\t0\nOneDrive Qualification\t3\t{old}\n", 10 * 86_400 - 60));
        assert_eq!(removable(&tasks, now, None), ["CodexQualification-a422e70", "Qualification-F90-TaskQ-Runtime"]);
        let units = linux_units(&format!("Id=taskq-stop-1.service\nStateChangeTimestampMonotonic={}\n\nId=taskq-qualification-x.service\n\
            StateChangeTimestampMonotonic={}\n\nId=other.service\nStateChangeTimestampMonotonic=1000000\n", 1_000_000, (5 * 86_400 - 60) as u64 * 1_000_000),
            5.0 * 86_400.0, now);
        assert_eq!(removable(&units, now, Some("taskq-")), ["taskq-stop-1.service"]);
        assert_eq!(mac_jobs("-\t0\tcom.taskq.qualification.f90\n123\t0\tcom.taskq.canary.live\n-\t0\tcom.taskq.service\n-\t0\tcom.apple.x\n"),
            ["com.taskq.qualification.f90"]);
        assert!(!qualification_object("TaskQ") && !qualification_object("taskq.service"));
    }
}
