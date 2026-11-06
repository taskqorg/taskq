//! Versioned tray and Conversations pages a running tray adopts without a binary update: docs/design/ui-bundle.md.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, RwLock},
};

const EMBEDDED_MANIFEST: &str = include_str!(concat!(env!("OUT_DIR"), "/web/manifest.json"));
const EMBEDDED_TRAY: &str = include_str!(concat!(env!("OUT_DIR"), "/web/tray.html"));
const EMBEDDED_WORKSPACE: &str = include_str!(concat!(env!("OUT_DIR"), "/web/workspace.html"));
const EMBEDDED_OPERATIONS: &str = include_str!(concat!(env!("OUT_DIR"), "/web/operations.json"));
/// Binary-injected policy: inlined code and `data:`/`blob:` media only, no connection, frame, form or base change (#2365 A6).
const CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval'; style-src 'unsafe-inline'; img-src data: blob:; media-src data: blob:; font-src data:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";
/// WebView2 `NavigateToString` refuses documents over 2 MB; the limit leaves room for the policy.
const MAX_PAGE: u64 = 2_000_000;
const MAX_OPERATIONS: u64 = 256 * 1024;
const MANIFEST_FILE: &str = "manifest.json";
const PAGE_FILES: [&str; 3] = ["tray.html", "workspace.html", "operations.json"];
const CHANNEL_TAG: &str = "ui-channel";

#[derive(Debug)]
pub struct Bundle {
    pub version: String,
    pub tray: String,
    pub workspace: String,
    operations: Vec<String>,
    pub embedded: bool,
}

#[derive(Default)]
struct Shared {
    active: Option<Arc<Bundle>>,
    reason: Option<String>,
    check_error: Option<String>,
}

static SHARED: RwLock<Shared> = RwLock::new(Shared {
    active: None,
    reason: None,
    check_error: None,
});
static EMBEDDED: LazyLock<Arc<Bundle>> = LazyLock::new(|| {
    let manifest: Manifest =
        serde_json::from_str(EMBEDDED_MANIFEST).expect("build-owned UI manifest");
    Arc::new(Bundle {
        version: manifest.bundle,
        tray: harden(EMBEDDED_TRAY).expect("build-owned tray page"),
        workspace: harden(EMBEDDED_WORKSPACE).expect("build-owned workspace page"),
        operations: serde_json::from_str(EMBEDDED_OPERATIONS).expect("build-owned web inventory"),
        embedded: true,
    })
});
static BRIDGE: LazyLock<u64> = LazyLock::new(|| {
    serde_json::from_str::<Manifest>(EMBEDDED_MANIFEST)
        .expect("build-owned UI manifest")
        .bridge
});

fn shared_read() -> std::sync::RwLockReadGuard<'static, Shared> {
    SHARED.read().unwrap_or_else(|e| e.into_inner())
}

fn shared_write() -> std::sync::RwLockWriteGuard<'static, Shared> {
    SHARED.write().unwrap_or_else(|e| e.into_inner())
}

pub fn embedded() -> Arc<Bundle> {
    EMBEDDED.clone()
}

/// The bundle new webviews load.
pub fn active() -> Arc<Bundle> {
    shared_read().active.clone().unwrap_or_else(embedded)
}

/// Web telemetry names: this binary's own inventory plus the active bundle's.
pub fn known_operation(name: &str) -> bool {
    let known = |bundle: &Bundle| bundle.operations.iter().any(|operation| operation == name);
    known(&EMBEDDED) || known(&active())
}

/// Facts for the page; the page writes the text.
pub fn status() -> Value {
    let shared = shared_read();
    let active = shared.active.clone().unwrap_or_else(embedded);
    json!({"bundle":active.version,"source":if active.embedded {"embedded"} else {"adopted"},"bridge":*BRIDGE,"reason":shared.reason,"check_error":shared.check_error})
}

/// Only binaries built by the release workflow follow the published channel, so a development build keeps its own pages.
pub fn follows_channel() -> bool {
    option_env!("TASKQ_UI_CHANNEL").is_some() || local_publication().is_some()
}

/// Debug-only local publication directory, laid out as `<tag>/<asset>`.
fn local_publication() -> Option<PathBuf> {
    cfg!(debug_assertions)
        .then(|| std::env::var_os("TASKQ_UI_RELEASE_DIR").map(PathBuf::from))
        .flatten()
}

/// Debug-only: a snapshot loads the store, so a bundle can be shown without a live tray.
pub fn snapshot_uses_store() -> bool {
    cfg!(debug_assertions) && std::env::var_os("TASKQ_UI_USE_STORE").is_some()
}

/// A higher epoch is newer; ties go by string order.
fn is_newer(a: &str, b: &str) -> bool {
    let epoch = |v: &str| {
        v.split('-')
            .next()
            .and_then(|e| e.parse::<u64>().ok())
            .unwrap_or(0)
    };
    (epoch(a), a) > (epoch(b), b)
}

fn valid_version(version: &str) -> bool {
    version.split_once('-').is_some_and(|(epoch, id)| {
        !epoch.is_empty()
            && epoch.len() <= 20
            && epoch.bytes().all(|b| b.is_ascii_digit())
            && !id.is_empty()
            && id.len() <= 40
            && id
                .bytes()
                .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
    })
}

/// Every page source gets the same policy; a page with no `<head>` could put content before it.
fn harden(page: &str) -> Result<String, String> {
    let at = page
        .find("<head>")
        .map(|at| at + "<head>".len())
        .ok_or("page has no <head>")?;
    Ok(format!(
        "{}<meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">{}",
        &page[..at],
        &page[at..]
    ))
}

#[derive(Deserialize)]
struct FileFact {
    sha256: String,
    size: u64,
}

#[derive(Deserialize)]
struct Manifest {
    schema: u64,
    bundle: String,
    bridge: u64,
    files: BTreeMap<String, FileFact>,
}

/// Verifies one bundle directory against its manifest and this binary's bridge version.
fn load_dir(dir: &Path, version: &str) -> Result<Bundle, String> {
    let manifest = fs::read(dir.join(MANIFEST_FILE)).map_err(|_| "manifest missing".to_string())?;
    let manifest: Manifest =
        serde_json::from_slice(&manifest).map_err(|_| "manifest invalid".to_string())?;
    if manifest.schema != 1 {
        return Err(format!("manifest schema {} unsupported", manifest.schema));
    }
    if manifest.bundle != version {
        return Err("manifest names another bundle".into());
    }
    if manifest.bridge != *BRIDGE {
        return Err(format!(
            "bridge {} incompatible, tray needs {}",
            manifest.bridge, *BRIDGE
        ));
    }
    let read = |name: &str, limit: u64| -> Result<Vec<u8>, String> {
        let fact = manifest
            .files
            .get(name)
            .ok_or_else(|| format!("manifest lists no {name}"))?;
        if fact.size > limit {
            return Err(format!("{name} too large"));
        }
        let bytes = fs::read(dir.join(name)).map_err(|_| format!("{name} missing"))?;
        if bytes.len() as u64 != fact.size
            || coreq::update::verify(format!("{}  {name}", fact.sha256).as_bytes(), name, &bytes)
                .is_err()
        {
            return Err(format!("{name} checksum wrong"));
        }
        Ok(bytes)
    };
    let page = |name: &str| -> Result<String, String> {
        let page =
            String::from_utf8(read(name, MAX_PAGE)?).map_err(|_| format!("{name} is not text"))?;
        harden(&page).map_err(|reason| format!("{name}: {reason}"))
    };
    let tray = page("tray.html")?;
    let workspace = page("workspace.html")?;
    let operations = serde_json::from_slice(&read("operations.json", MAX_OPERATIONS)?)
        .map_err(|_| "operations.json invalid".to_string())?;
    Ok(Bundle {
        version: version.into(),
        tray,
        workspace,
        operations,
        embedded: false,
    })
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    current: Option<String>,
    previous: Option<String>,
    #[serde(default)]
    bad: BTreeMap<String, String>,
}

/// What a background check found; the event loop applies it to the store.
#[derive(Clone, Debug, PartialEq)]
pub enum Checked {
    Current,
    Ready(String),
    Refused { version: String, reason: String },
    Failed(String),
}

/// A check's inputs, copied so it can run off the event loop.
#[derive(Clone)]
pub struct Check {
    root: PathBuf,
    active: String,
    bad: Vec<String>,
}

fn pointer_name() -> String {
    format!("ui-latest-b{}.json", *BRIDGE)
}

fn asset_name(version: &str, file: &str) -> String {
    format!("taskq-ui-{version}-{file}")
}

impl Check {
    /// Fetches the channel pointer and, for a newer compatible bundle, downloads and verifies it into the store.
    pub fn run(&self, fetch: &dyn Fn(&str, &str, &Path) -> Result<(), String>) -> Checked {
        let scratch = self.root.join(format!(
            ".tmp-{}-{}",
            std::process::id(),
            crate::runtime::uuid()
        ));
        let checked = self.run_in(&scratch, fetch);
        let _ = fs::remove_dir_all(&scratch);
        checked
    }

    fn run_in(
        &self,
        scratch: &Path,
        fetch: &dyn Fn(&str, &str, &Path) -> Result<(), String>,
    ) -> Checked {
        if let Err(e) = fs::create_dir_all(scratch) {
            return Checked::Failed(format!("UI store unavailable: {e}"));
        }
        let pointer_path = scratch.join("pointer.json");
        if let Err(e) = fetch(CHANNEL_TAG, &pointer_name(), &pointer_path) {
            return Checked::Failed(e);
        }
        let pointer: Value = fs::read(&pointer_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let version = pointer["bundle"].as_str().unwrap_or_default().to_string();
        let tag = format!("ui-{version}");
        // A pointer is only a name: an invalid one names no directory or URL and is retried at the next check.
        if !(pointer["schema"] == 1
            && pointer["bridge"] == *BRIDGE
            && pointer["tag"] == tag.as_str()
            && valid_version(&version))
        {
            return Checked::Failed("UI channel pointer invalid".into());
        }
        if !is_newer(&version, &self.active) || self.bad.contains(&version) {
            return Checked::Current;
        }
        let refused = |reason: String| Checked::Refused {
            version: version.clone(),
            reason,
        };
        let staged = scratch.join("bundle");
        if let Err(e) = fs::create_dir_all(&staged) {
            return Checked::Failed(format!("UI store unavailable: {e}"));
        }
        let sums_path = scratch.join("SHA256SUMS");
        if let Err(e) = fetch(&tag, "SHA256SUMS", &sums_path) {
            return Checked::Failed(e);
        }
        let Ok(sums) = fs::read(&sums_path) else {
            return Checked::Failed("SHA256SUMS unreadable".into());
        };
        for file in [MANIFEST_FILE].into_iter().chain(PAGE_FILES) {
            let asset = asset_name(&version, file);
            let to = staged.join(file);
            if let Err(e) = fetch(&tag, &asset, &to) {
                return Checked::Failed(e);
            }
            let Ok(bytes) = fs::read(&to) else {
                return Checked::Failed(format!("{file} unreadable"));
            };
            if let Err(e) = coreq::update::verify(&sums, &asset, &bytes) {
                return refused(format!("checksum wrong: {e}"));
            }
        }
        if let Err(reason) = load_dir(&staged, &version) {
            return refused(reason);
        }
        let target = self.root.join(&version);
        let _ = fs::remove_dir_all(&target);
        if let Err(e) = fs::rename(&staged, &target) {
            return Checked::Failed(format!("UI store unavailable: {e}"));
        }
        Checked::Ready(version)
    }
}

/// Production fetch: HTTPS to the release repository (CoreQ R11); a debug build can read a local publication.
pub fn fetch(tag: &str, name: &str, to: &Path) -> Result<(), String> {
    if let Some(dir) = local_publication() {
        return fs::copy(dir.join(tag).join(name), to)
            .map(|_| ())
            .map_err(|e| format!("{tag}/{name}: {e}"));
    }
    coreq::update::download(
        &format!(
            "https://github.com/{}/releases/download/{tag}/{name}",
            crate::platform::RELEASE_REPO
        ),
        to,
    )
}

/// The on-disk store: `<install>/ui/<bundle>/` and `<install>/ui/state.json`.
pub struct Store {
    root: PathBuf,
    state: State,
}

impl Store {
    /// Picks the newest verified copy and makes it active; a stored bundle not newer than this binary's own is ignored.
    pub fn open(install: &Path) -> Store {
        let root = install.join("ui");
        let state = fs::read(root.join("state.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut store = Store { root, state };
        if let Ok(entries) = fs::read_dir(&store.root) {
            for entry in entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp-"))
            {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
        store.pick();
        store
    }

    /// Activates current, previous or embedded; a stored copy that fails verification is marked bad with a visible reason.
    fn pick(&mut self) {
        let embedded = embedded();
        let mut chosen = embedded.clone();
        let mut damaged = false;
        for version in [self.state.current.clone(), self.state.previous.clone()]
            .into_iter()
            .flatten()
        {
            if !valid_version(&version)
                || !is_newer(&version, &embedded.version)
                || self.state.bad.contains_key(&version)
            {
                continue;
            }
            match load_dir(&self.root.join(&version), &version) {
                Ok(bundle) => {
                    chosen = Arc::new(bundle);
                    break;
                }
                Err(why) => {
                    self.state.bad.insert(version, why);
                    damaged = true;
                }
            }
        }
        if damaged {
            self.save();
        }
        let reason = self.standing_reason(&chosen.version);
        let mut shared = shared_write();
        shared.active = Some(chosen);
        shared.reason = reason;
    }

    /// The newest refusal above the running copy: it stays visible, across a restart too, until a newer good bundle replaces it.
    fn standing_reason(&self, using: &str) -> Option<String> {
        let mut newest: Option<(&String, &String)> = None;
        for (version, why) in self
            .state
            .bad
            .iter()
            .filter(|(version, _)| is_newer(version, using))
        {
            if newest.is_none_or(|(best, _)| is_newer(version, best)) {
                newest = Some((version, why));
            }
        }
        newest.map(|(version, why)| refused_text(version, why, using))
    }

    pub fn check(&self) -> Check {
        Check {
            root: self.root.clone(),
            active: active().version.clone(),
            bad: self.state.bad.keys().cloned().collect(),
        }
    }

    /// Applies a check result; a ready bundle becomes active on trial, so `current` stays until it proves itself.
    pub fn apply(&mut self, checked: Checked) {
        match checked {
            Checked::Current => shared_write().check_error = None,
            Checked::Failed(error) => shared_write().check_error = Some(error),
            Checked::Refused { version, reason } => {
                self.mark_bad(&version, &reason);
                let standing = self.standing_reason(&active().version);
                let mut shared = shared_write();
                shared.check_error = None;
                shared.reason = standing;
            }
            Checked::Ready(version) => match load_dir(&self.root.join(&version), &version) {
                Ok(bundle) => {
                    let standing = self.standing_reason(&bundle.version);
                    let mut shared = shared_write();
                    shared.active = Some(Arc::new(bundle));
                    shared.check_error = None;
                    shared.reason = standing;
                }
                Err(reason) => shared_write().check_error = Some(reason),
            },
        }
    }

    /// The active bundle proved itself: it becomes `current` and the replaced `current` becomes `previous`.
    pub fn commit(&mut self) {
        let active = active();
        if active.embedded || self.state.current.as_deref() == Some(&active.version) {
            return;
        }
        self.state.previous = self.state.current.replace(active.version.clone());
        self.state
            .bad
            .retain(|version, _| is_newer(version, &active.version));
        self.save();
        self.prune(&active.version);
    }

    /// Whether the active bundle still has to prove itself.
    pub fn on_trial(&self) -> bool {
        let active = active();
        !active.embedded && self.state.current.as_deref() != Some(&active.version)
    }

    /// The bundle failed: it is never retried, and the previous good or embedded copy takes over with a visible reason.
    pub fn fail(&mut self, version: &str, reason: &str) -> bool {
        if embedded().version == version {
            return false;
        }
        self.mark_bad(version, reason);
        let was_active = active().version == version;
        if was_active {
            self.pick();
        }
        shared_write().reason = self.standing_reason(&active().version);
        was_active
    }

    fn mark_bad(&mut self, version: &str, reason: &str) {
        self.state.bad.insert(version.into(), reason.into());
        if self.state.current.as_deref() == Some(version) {
            self.state.current = self.state.previous.take();
        }
        if self.state.previous.as_deref() == Some(version) {
            self.state.previous = None;
        }
        self.save();
        let _ = fs::remove_dir_all(self.root.join(version));
    }

    fn save(&self) {
        let _ = fs::create_dir_all(&self.root);
        let text = serde_json::to_string_pretty(&self.state).unwrap_or_default();
        if let Err(e) = coreq::write_atomic(&self.root.join("state.json"), &text) {
            shared_write().check_error = Some(format!("UI store unavailable: {e}"));
        }
    }

    /// Keeps the running, current and previous bundles and anything newer, which an in-flight check may have just staged.
    fn prune(&self, running: &str) {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        for entry in entries.flatten().filter(|e| e.path().is_dir()) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let keep = !valid_version(&name)
                || name == running
                || self.state.current.as_deref() == Some(&name)
                || self.state.previous.as_deref() == Some(&name)
                || is_newer(&name, running);
            if !keep {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}

fn refused_text(version: &str, reason: &str, using: &str) -> String {
    format!("UI bundle {version} refused: {reason}; using {using}")
}

#[cfg(test)]
#[path = "ui_bundle_tests.rs"]
mod tests;
