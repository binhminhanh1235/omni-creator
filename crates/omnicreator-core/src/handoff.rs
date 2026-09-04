use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Error;
use crate::{
    fs_util::{atomic_write_json, sha256_file},
    LogicalUri, PathResolver, Result, StateStore, Workspace,
};

pub const HANDOFF_SCHEMA: &str = "omnicreator.handoff";
pub const HANDOFF_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffManifest {
    pub schema: String,
    pub schema_version: u32,
    pub workspace_id: String,
    pub revision: u64,
    pub snapshot_uri: LogicalUri,
    pub snapshot_sha256: String,
    pub snapshot_size_bytes: u64,
    pub created_at: DateTime<Utc>,
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryOutcome {
    Healthy,
    Recovered { revision: u64 },
}

pub(crate) fn create_handoff(
    workspace: &mut Workspace,
    state_store: &StateStore,
    device_id: &str,
    retained_snapshots: usize,
) -> Result<HandoffManifest> {
    state_store.integrity_check()?;

    let backup_dir = workspace.data_root().join(".omnicreator/backups");
    fs::create_dir_all(&backup_dir)?;

    let mut revision = workspace.manifest().revision + 1;
    let final_snapshot = loop {
        let candidate = backup_dir.join(format!("state-r{revision:06}.sqlite"));
        if !candidate.exists() {
            break candidate;
        }
        revision += 1;
    };

    let temp_snapshot = backup_dir.join(format!(
        ".state-r{revision:06}-{}.tmp",
        Uuid::new_v4().simple()
    ));
    state_store.create_snapshot(&temp_snapshot)?;
    let (snapshot_sha256, snapshot_size_bytes) = sha256_file(&temp_snapshot)?;

    fs::rename(&temp_snapshot, &final_snapshot)?;

    let relative = final_snapshot
        .strip_prefix(workspace.data_root())
        .map_err(|_| Error::InvalidHandoff("snapshot escaped Data Root".to_owned()))?;
    let logical_path = relative
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let snapshot_uri = LogicalUri::parse(&format!("workspace://{logical_path}"))?;

    let handoff = HandoffManifest {
        schema: HANDOFF_SCHEMA.to_owned(),
        schema_version: HANDOFF_SCHEMA_VERSION,
        workspace_id: workspace.manifest().workspace_id.clone(),
        revision,
        snapshot_uri,
        snapshot_sha256,
        snapshot_size_bytes,
        created_at: Utc::now(),
        device_id: device_id.to_owned(),
    };

    atomic_write_json(
        &workspace
            .data_root()
            .join(".omnicreator/handoff/latest.json"),
        &handoff,
    )?;
    workspace.mark_handoff_clean(device_id, revision)?;
    rotate_snapshots(&backup_dir, retained_snapshots.max(1), &final_snapshot)?;

    Ok(handoff)
}

impl Workspace {
    pub fn validate_latest_handoff(&self) -> Result<HandoffManifest> {
        let handoff_path = self.data_root().join(".omnicreator/handoff/latest.json");
        if !handoff_path.exists() {
            return Err(Error::InvalidHandoff(
                "latest handoff manifest does not exist".to_owned(),
            ));
        }

        let handoff: HandoffManifest = serde_json::from_slice(&fs::read(handoff_path)?)?;
        if handoff.schema != HANDOFF_SCHEMA
            || handoff.schema_version != HANDOFF_SCHEMA_VERSION
            || handoff.workspace_id != self.manifest().workspace_id
        {
            return Err(Error::InvalidHandoff(
                "handoff manifest is incompatible with this workspace".to_owned(),
            ));
        }

        let resolver = PathResolver::new(self.data_root())?;
        let snapshot_path = resolver.resolve(&handoff.snapshot_uri, None)?;
        let (sha256, size) = sha256_file(&snapshot_path)?;
        if sha256 != handoff.snapshot_sha256 || size != handoff.snapshot_size_bytes {
            return Err(Error::InvalidHandoff(
                "snapshot hash or size does not match handoff manifest".to_owned(),
            ));
        }
        StateStore::validate_database(&snapshot_path)?;
        Ok(handoff)
    }

    pub fn recover_if_needed(&self) -> Result<RecoveryOutcome> {
        let sqlite_path = self.sqlite_path();
        if sqlite_path.exists() && StateStore::validate_database(&sqlite_path).is_ok() {
            return Ok(RecoveryOutcome::Healthy);
        }

        let handoff = self.validate_latest_handoff()?;
        let resolver = PathResolver::new(self.data_root())?;
        let snapshot_path = resolver.resolve(&handoff.snapshot_uri, None)?;
        restore_snapshot(&snapshot_path, &sqlite_path)?;
        StateStore::validate_database(&sqlite_path)?;

        Ok(RecoveryOutcome::Recovered {
            revision: handoff.revision,
        })
    }
}

fn restore_snapshot(snapshot: &Path, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| Error::InvalidHandoff("state database has no parent".to_owned()))?;
    fs::create_dir_all(parent)?;

    let temp = parent.join(format!(".recovery-{}.sqlite", Uuid::new_v4().simple()));
    fs::copy(snapshot, &temp)?;
    {
        let mut file = fs::OpenOptions::new().append(true).open(&temp)?;
        file.flush()?;
        file.sync_all()?;
    }

    let quarantine = parent.join(format!(".corrupt-{}.sqlite", Uuid::new_v4().simple()));
    let had_destination = destination.exists();
    if had_destination {
        fs::rename(destination, &quarantine)?;
    }

    if let Err(error) = fs::rename(&temp, destination) {
        let _ = fs::remove_file(&temp);
        if had_destination {
            let _ = fs::rename(&quarantine, destination);
        }
        return Err(error.into());
    }

    if let Err(error) = StateStore::validate_database(destination) {
        let _ = fs::remove_file(destination);
        if had_destination {
            let _ = fs::rename(&quarantine, destination);
        }
        return Err(error);
    }

    if had_destination {
        fs::remove_file(quarantine)?;
    }
    Ok(())
}

fn rotate_snapshots(backup_dir: &Path, retain: usize, current: &Path) -> Result<()> {
    let mut snapshots = fs::read_dir(backup_dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("state-r") && name.ends_with(".sqlite"))
        })
        .collect::<Vec<PathBuf>>();
    snapshots.sort();

    let removable = snapshots.len().saturating_sub(retain);
    for path in snapshots.into_iter().take(removable) {
        if path != current {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StateStore;

    #[test]
    fn clean_handoff_can_recover_a_corrupted_working_database() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        let mut workspace = Workspace::create(&root).unwrap();

        let writer = workspace.acquire_writer("device-a").unwrap();
        let sqlite_path = writer.sqlite_path();
        let store = StateStore::open(&sqlite_path).unwrap();
        let project = store.create_project("Recover Me").unwrap();

        let handoff = writer.prepare_handoff(&store, 3).unwrap();
        drop(store);

        assert!(workspace.manifest().last_clean_shutdown);
        assert_eq!(workspace.manifest().revision, handoff.revision);
        workspace.validate_latest_handoff().unwrap();

        fs::write(&sqlite_path, b"definitely not sqlite").unwrap();
        assert!(StateStore::validate_database(&sqlite_path).is_err());

        assert_eq!(
            workspace.recover_if_needed().unwrap(),
            RecoveryOutcome::Recovered {
                revision: handoff.revision
            }
        );

        let restored = StateStore::open(&sqlite_path).unwrap();
        assert_eq!(
            restored.get_project(&project.id).unwrap().title,
            "Recover Me"
        );
    }

    #[test]
    fn handoff_rotation_retains_configured_number_of_snapshots() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        let mut workspace = Workspace::create(&root).unwrap();

        for index in 0..4 {
            let writer = workspace.acquire_writer("device-a").unwrap();
            let store = StateStore::open(writer.sqlite_path()).unwrap();
            store.create_project(&format!("Project {index}")).unwrap();
            writer.prepare_handoff(&store, 2).unwrap();
        }

        let count = fs::read_dir(root.join(".omnicreator/backups"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("state-r") && name.ends_with(".sqlite"))
            })
            .count();
        assert_eq!(count, 2);
    }
}
