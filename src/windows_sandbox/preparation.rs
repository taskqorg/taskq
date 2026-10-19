//! Durable security preimages; mutable file contents are never backed up or restored here.
use super::security::Snapshot;
use std::{fs, os::windows::fs::OpenOptionsExt, path::{Path, PathBuf}};

const DATABASES: &[&str] = &["state_5.sqlite", "logs_2.sqlite", "goals_1.sqlite", "memories_1.sqlite", "memories_v2_1.sqlite", "queue_1.sqlite", "thread_history_1.sqlite"];
const DATA_DIRS: &[&str] = &["sessions", "archived_sessions", "log", "shell_snapshots", "memories", "tmp"];

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Record { original: Snapshot, previous: Snapshot, applied: Snapshot }
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal { version: u32, phase: String, records: Vec<Record> }

pub(super) struct Preparation {
    file: PathBuf,
    journal: Journal,
    targets: Vec<(usize, bool, bool)>,
    home: PathBuf,
    _lock: fs::File,
}

fn tree(path: &Path, low: bool, protected: Option<&Path>, shallow: Option<&Path>, out: &mut Vec<(PathBuf, bool, bool)>) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    // Links are not writable-root authority. Do not follow or relabel their targets.
    if metadata.file_attributes() & 0x400 != 0 { return Ok(()); }
    let low = low && protected != Some(path);
    out.push((path.to_owned(), low, false));
    if metadata.is_dir() && shallow != Some(path) {
        let mut children = fs::read_dir(path).map_err(|e| e.to_string())?.map(|e| e.map(|e| e.path())).collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        children.sort();
        for child in children { tree(&child, low, protected, shallow, out)?; }
    }
    Ok(())
}

fn create_file(path: &Path) -> Result<(), String> {
    match fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => { super::checked(path)?; if path.is_file() { Ok(()) } else { Err("Windows mutable state is not a file".into()) } },
        Err(e) => Err(e.to_string()),
    }
}

impl Preparation {
    fn save(&self) -> Result<(), String> {
        crate::platform::write_atomic(&self.file, &serde_json::to_string(&self.journal).map_err(|e| e.to_string())?)
    }

    pub fn cancel(mut self) -> Result<(), String> {
        for (index, _, _) in &self.targets {
            let record = &mut self.journal.records[*index];
            let current = Snapshot::capture(&record.original.path)?;
            if current.volume != record.original.volume || current.file_id != record.original.file_id { return Err("Windows security identity changed during failed preflight; journal retained".into()); }
            record.previous = current.clone(); record.applied = current;
        }
        self.journal.phase = "complete".into(); self.save()
    }

    /// Called before the SDK preflight changes any root DACL.
    pub fn begin(root: &Path, install: &Path, home: &Path, state: &Path, temp: &Path, binaries: &[&Path]) -> Result<Self, String> {
        let root = super::checked(root)?; let install = super::checked(install)?; let home = super::checked(home)?;
        let state = super::checked(state)?; let temp = super::checked(temp)?;
        let (root, install, home, state, temp) = (root.as_path(), install.as_path(), home.as_path(), state.as_path(), temp.as_path());
        for (a, b) in [(root, install), (root, home), (root, temp), (install, home), (temp, home), (install, temp)] {
            if a.starts_with(b) || b.starts_with(a) { return Err("Windows security: workspace, installation, home and temp must be disjoint".into()); }
        }
        let file = install.join("windows-native/security-preparation.json");
        let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).share_mode(0)
            .open(file.with_extension("lock")).map_err(|e| format!("Windows security preparation busy or unavailable: {e}"))?;
        let journal = match fs::read(&file) {
            Ok(bytes) => serde_json::from_slice::<Journal>(&bytes).map_err(|e| e.to_string())?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Journal { version: 1, phase: "complete".into(), records: vec![] },
            Err(e) => return Err(e.to_string()),
        };
        if journal.version != 1 || journal.phase != "complete" { return Err("Windows security preparation incomplete; keep journal for identity-bound recovery".into()); }
        let mut scopes = Vec::new();
        let protected = super::checked(&root.join(".codex"))?;
        tree(root, true, Some(&protected), None, &mut scopes)?;
        tree(temp, true, None, None, &mut scopes)?;
        scopes.push((home.to_owned(), false, false));
        scopes.push((install.to_owned(), false, false));
        // SDK preflight replaces generated tmp children. Journal the Medium boundary,
        // whose label new children inherit, without pinning those volatile identities.
        let state_temp = state.join("tmp");
        super::security::create_directory(&state_temp)?;
        let state_temp = super::checked(&state_temp)?;
        tree(state, false, None, Some(&state_temp), &mut scopes)?;
        for path in binaries {
            let path = super::checked(path)?;
            if !path.starts_with(install) { return Err("Windows bootstrap/dependencies must be inside the protected installation".into()); }
            scopes.push((path, false, false));
        }
        for name in DATA_DIRS {
            let path = home.join(name);
            super::security::create_directory(&path)?;
            super::checked(&path)?;
            tree(&path, true, None, None, &mut scopes)?;
        }
        for name in DATABASES {
            for suffix in ["", "-wal", "-shm"] {
                let path = home.join(format!("{name}{suffix}")); create_file(&path)?;
                scopes.push((path, true, !suffix.is_empty()));
            }
        }
        // Empty JSONL and advisory-lock files are valid; JSON caches are never synthesized.
        for name in ["session_index.jsonl", "history.jsonl", ".sqlite-maintenance.lock"] {
            let path = home.join(name); create_file(&path)?; scopes.push((path, true, false));
        }
        for entry in fs::read_dir(home).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if super::control(&path) { tree(&path, false, None, None, &mut scopes)?; }
        }
        let mut result = Self { file, journal, targets: vec![], home: home.to_owned(), _lock: lock };
        for (path, low, sidecar) in scopes {
            let current = Snapshot::capture(&path)?;
            let index = result.journal.records.iter().position(|r| r.original.path == path && r.original.volume == current.volume && r.original.file_id == current.file_id);
            let index = if let Some(index) = index {
                if result.journal.records[index].applied != current { return Err(format!("Windows security changed outside preparation at {}", path.display())); }
                index
            } else {
                result.journal.records.push(Record { original: current.clone(), previous: current.clone(), applied: current });
                result.journal.records.len() - 1
            };
            result.targets.push((index, low, sidecar));
        }
        result.journal.phase = "preflight".into(); result.save()?;
        Ok(result)
    }

    pub fn apply(mut self) -> Result<(), String> {
        for (index, low, sidecar) in &self.targets {
            let record = &mut self.journal.records[*index];
            let current = Snapshot::capture(&record.original.path)?;
            if current.volume != record.original.volume || current.file_id != record.original.file_id || current.directory != record.original.directory {
                return Err("Windows security identity changed during native preflight".into());
            }
            let mut target = if *low { current.low()? } else { current.medium()? };
            if current.path == self.home { target = target.deny(0x40, "S-1-1-0")?; }
            // Keep WAL/SHM through every client's last close; the owner can restore the journal.
            if *sidecar { target = target.deny(0x10000, "S-1-1-0")?; }
            record.previous = current; record.applied = target;
        }
        self.journal.phase = "apply".into(); self.save()?;
        for (index, _, _) in &self.targets {
            let record = &self.journal.records[*index];
            if record.previous != record.applied { record.previous.apply(&record.applied)?; }
            if Snapshot::capture(&record.applied.path)? != record.applied { return Err("Windows security path readback changed; journal retained".into()); }
        }
        self.journal.phase = "complete".into(); self.save()
    }
}

pub(super) fn ready(install: &Path) -> Result<(), String> {
    let path = install.join("windows-native/security-preparation.json");
    match fs::read(&path) {
        Ok(bytes) => {
            super::checked(&path)?;
            let journal: Journal = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if journal.version != 1 || journal.phase != "complete" { return Err("Windows security preparation incomplete; restore before admission".into()); }
            Ok(())
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Restore only saved object security. Missing/replaced files and all file contents are preserved.
pub(super) fn restore(install: &Path) -> Result<(), String> {
    if super::restricted()? || std::env::var_os("TASKQ_ATTEMPT").is_some() { return Err("Windows security restore requires an unrestricted operator".into()); }
    let file = install.join("windows-native/security-preparation.json");
    let lock = fs::OpenOptions::new().read(true).write(true).share_mode(0).open(file.with_extension("lock")).map_err(|e| e.to_string())?;
    let journal: Journal = serde_json::from_slice(&fs::read(&file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if journal.version != 1 { return Err("Windows security journal version unknown".into()); }
    let mut plan = Preparation { file, journal, targets: vec![], home: PathBuf::new(), _lock: lock };
    let mut pairs = Vec::new();
    for (i, record) in plan.journal.records.iter().enumerate().rev() {
        if !record.original.path.exists() { continue; }
        let current = Snapshot::capture(&record.original.path)?;
        if current.volume != record.original.volume || current.file_id != record.original.file_id { continue; }
        if current != record.applied && current != record.previous && current != record.original {
            return Err(format!("Windows security restore refuses concurrent changes at {}", current.path.display()));
        }
        pairs.push((i, current));
    }
    plan.journal.phase = "restore".into(); plan.save()?;
    for (i, current) in pairs {
        let record = &mut plan.journal.records[i];
        if current != record.original { current.apply(&record.original)?; }
        if Snapshot::capture(&current.path)? != record.original { return Err("Windows security restore path changed; journal retained".into()); }
        record.previous = record.original.clone(); record.applied = record.original.clone();
    }
    plan.journal.phase = "complete".into(); plan.save()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "native Windows security journal fixture"]
    fn native_preparation_tolerates_sdk_temp_cleanup() {
        let base = std::env::temp_dir().join(format!("taskq-preparation-temp-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let root = base.join("workspace"); let install = base.join("install"); let home = base.join("home");
        let state = install.join("windows-native/state"); let temp = base.join("temp");
        let state_temp = state.join("TMP"); let generated = state_temp.join("arg0/codex-arg0-fixture");
        for path in [root.join(".codex"), generated.clone(), home.clone(), temp.clone()] { fs::create_dir_all(path).unwrap(); }
        fs::write(generated.join(".lock"), "fixture lock").unwrap();
        fs::write(generated.join("apply_patch.bat"), "fixture patch helper").unwrap();
        let config = home.join("config.toml"); let control = state.join("control.json");
        fs::write(&config, "# invented fixture configuration\n").unwrap();
        fs::write(&control, "{}").unwrap();
        let before = [Snapshot::capture(&config).unwrap(), Snapshot::capture(&control).unwrap(), Snapshot::capture(&state_temp).unwrap()];
        let preparation = Preparation::begin(&root, &install, &home, &state, &temp, &[]).unwrap();
        let boundary = super::super::checked(&state_temp).unwrap();
        assert!(preparation.journal.records.iter().any(|r| r.original.path == boundary));
        assert!(!preparation.journal.records.iter().any(|r| r.original.path != boundary && r.original.path.starts_with(&boundary)));
        for original in &before {
            assert!(preparation.journal.records.iter().any(|r| r.original == *original));
        }
        // The SDK legitimately removes previous generated helpers during preflight.
        fs::remove_dir_all(state_temp.join("arg0")).unwrap();
        preparation.apply().unwrap();
        for path in [&state, &state_temp, &config, &control] {
            assert!(Snapshot::capture(path).unwrap().descriptor.contains(";;;ME)"));
        }
        let fresh = state_temp.join("new-helper.bat"); fs::write(&fresh, "new SDK helper").unwrap();
        assert!(Snapshot::capture(&fresh).unwrap().descriptor.contains(";;;ME)"));
        fs::write(&config, "# owner changes survive rollback\n").unwrap();
        restore(&install).unwrap();
        for original in before { assert_eq!(Snapshot::capture(&original.path).unwrap(), original); }
        assert_eq!(fs::read_to_string(&config).unwrap(), "# owner changes survive rollback\n");
        assert_eq!(fs::read_to_string(&fresh).unwrap(), "new SDK helper");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn native_preparation_and_restore_preserve_controls_and_data() {
        let base = std::env::temp_dir().join(format!("taskq-preparation-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let root = base.join("workspace"); let install = base.join("install"); let home = base.join("home");
        let state = install.join("windows-native/state"); let temp = base.join("temp");
        for path in [root.join(".CODEX"), state.clone(), home.clone(), temp.clone()] { fs::create_dir_all(path).unwrap(); }
        let overlapping = install.join("workspace"); fs::create_dir(&overlapping).unwrap();
        assert!(Preparation::begin(&overlapping, &install, &home, &state, &temp, &[]).is_err());
        assert!(!install.join("windows-native/security-preparation.json").exists(), "overlap refusal precedes effects");
        fs::write(home.join("config.TOML"), "# invented fixture configuration\n").unwrap();
        fs::write(root.join(".codex/control.toml"), "# invented fixture control\n").unwrap();
        fs::write(root.join("data.txt"), "before").unwrap();
        let before = Snapshot::capture(&root).unwrap();
        let home_before = Snapshot::capture(&home).unwrap();
        Preparation::begin(&root, &install, &home, &state, &temp, &[]).unwrap().apply().unwrap();
        assert_eq!(Snapshot::capture(&root).unwrap(), before.low().unwrap());
        assert!(Snapshot::capture(&root.join(".codex/control.toml")).unwrap().descriptor.contains(";;;ME)"));
        assert!(Snapshot::capture(&home.join("config.toml")).unwrap().descriptor.contains(";;;ME)"));
        fs::write(root.join("data.txt"), "owner data survives rollback").unwrap();
        fs::write(home.join("owner-config.tmp"), "owner rewrite").unwrap();
        fs::rename(home.join("owner-config.tmp"), home.join("config.toml")).unwrap();
        let wal = home.join("state_5.sqlite-wal");
        assert_eq!(fs::remove_file(&wal).unwrap_err().raw_os_error(), Some(5));
        // Replacing an owner control is a conflict, not permission to restore over its new identity.
        restore(&install).unwrap();
        assert_eq!(Snapshot::capture(&root).unwrap(), before);
        assert_eq!(Snapshot::capture(&home).unwrap(), home_before);
        assert_eq!(fs::read_to_string(root.join("data.txt")).unwrap(), "owner data survives rollback");
        assert_eq!(fs::read_to_string(home.join("config.toml")).unwrap(), "owner rewrite");
        fs::remove_file(&wal).unwrap();
        fs::remove_dir_all(&base).unwrap();
    }
}
