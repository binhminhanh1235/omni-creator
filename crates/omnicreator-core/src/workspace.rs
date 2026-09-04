use std::{
    fs,
    fs::File,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Duration, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{fs_util::atomic_write_json, handoff::HandoffManifest, Error, Result, StateStore};

pub const WORKSPACE_SCHEMA: &str = "omnicreator.workspace";
pub const WORKSPACE_SCHEMA_VERSION: u32 = 1;
pub const WRITER_LEASE_SCHEMA: &str = "omnicreator.writer-lease";
pub const WRITER_LEASE_SCHEMA_VERSION: u32 = 1;
pub const WRITER_LEASE_TTL_SECONDS: i64 = 120;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceManifest {
    pub schema: String,
    pub schema_version: u32,
    pub workspace_id: String,
    pub revision: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_clean_shutdown: bool,
    pub last_writer_device: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WriterLease {
    pub schema: String,
    pub schema_version: u32,
    pub workspace_id: String,
    pub device_id: String,
    pub session_id: String,
    pub updated_at: DateTime<Utc>,
}

impl WriterLease {
    fn is_recent(&self) -> bool {
        Utc::now().signed_duration_since(self.updated_at)
            < Duration::seconds(WRITER_LEASE_TTL_SECONDS)
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    data_root: PathBuf,
    manifest: WorkspaceManifest,
}

pub struct WorkspaceWriter<'a> {
    pub(crate) workspace: &'a mut Workspace,
    lock_file: File,
    pub(crate) lease: WriterLease,
    released: bool,
}

impl Workspace {
    pub fn create(data_root: impl AsRef<Path>) -> Result<Self> {
        let data_root = data_root.as_ref();
        fs::create_dir_all(data_root)?;
        let manifest_path = data_root.join(".omnicreator/workspace.json");
        if manifest_path.exists() {
            return Err(Error::WorkspaceAlreadyExists(data_root.to_path_buf()));
        }

        create_layout(data_root)?;
        let now = Utc::now();
        let manifest = WorkspaceManifest {
            schema: WORKSPACE_SCHEMA.to_owned(),
            schema_version: WORKSPACE_SCHEMA_VERSION,
            workspace_id: format!("ws_{}", Uuid::new_v4().simple()),
            revision: 0,
            created_at: now,
            updated_at: now,
            last_clean_shutdown: true,
            last_writer_device: None,
        };
        atomic_write_json(&manifest_path, &manifest)?;

        Ok(Self {
            data_root: fs::canonicalize(data_root)?,
            manifest,
        })
    }

    pub fn open(data_root: impl AsRef<Path>) -> Result<Self> {
        let data_root = fs::canonicalize(data_root)?;
        let manifest_path = data_root.join(".omnicreator/workspace.json");
        let manifest: WorkspaceManifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
        validate_manifest(&manifest)?;
        create_layout(&data_root)?;

        Ok(Self {
            data_root,
            manifest,
        })
    }

    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    pub fn manifest(&self) -> &WorkspaceManifest {
        &self.manifest
    }

    pub fn sqlite_path(&self) -> PathBuf {
        self.data_root.join(".omnicreator/state/omnicreator.sqlite")
    }

    pub fn acquire_writer(&mut self, device_id: &str) -> Result<WorkspaceWriter<'_>> {
        if device_id.is_empty() {
            return Err(Error::InvalidWorkspace(
                "device_id is required for writer acquisition".to_owned(),
            ));
        }

        let lock_path = self.data_root.join(".omnicreator/writer.lock");
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(lock_path)?;

        if FileExt::try_lock_exclusive(&lock_file).is_err() {
            return Err(Error::WorkspaceBusy(
                "another local process already owns the workspace lock".to_owned(),
            ));
        }

        let lease_path = self.lease_path();
        let result = (|| {
            if lease_path.exists() {
                let existing: WriterLease = serde_json::from_slice(&fs::read(&lease_path)?)?;
                validate_lease(&existing, &self.manifest.workspace_id)?;
                if existing.is_recent() && existing.device_id != device_id {
                    return Err(Error::WorkspaceBusy(format!(
                        "recent writer lease belongs to device {}",
                        existing.device_id
                    )));
                }
            }

            let lease = WriterLease {
                schema: WRITER_LEASE_SCHEMA.to_owned(),
                schema_version: WRITER_LEASE_SCHEMA_VERSION,
                workspace_id: self.manifest.workspace_id.clone(),
                device_id: device_id.to_owned(),
                session_id: format!("session_{}", Uuid::new_v4().simple()),
                updated_at: Utc::now(),
            };
            atomic_write_json(&lease_path, &lease)?;
            self.mark_writer_open(device_id)?;
            Ok(lease)
        })();

        match result {
            Ok(lease) => Ok(WorkspaceWriter {
                workspace: self,
                lock_file,
                lease,
                released: false,
            }),
            Err(error) => {
                let _ = FileExt::unlock(&lock_file);
                Err(error)
            }
        }
    }

    pub(crate) fn mark_handoff_clean(&mut self, device_id: &str, revision: u64) -> Result<()> {
        if revision <= self.manifest.revision {
            return Err(Error::InvalidHandoff(format!(
                "handoff revision {revision} is not newer than workspace revision {}",
                self.manifest.revision
            )));
        }
        self.manifest.revision = revision;
        self.manifest.last_clean_shutdown = true;
        self.manifest.last_writer_device = Some(device_id.to_owned());
        self.manifest.updated_at = Utc::now();
        self.persist_manifest()
    }

    fn mark_writer_open(&mut self, device_id: &str) -> Result<()> {
        self.manifest.last_clean_shutdown = false;
        self.manifest.last_writer_device = Some(device_id.to_owned());
        self.manifest.updated_at = Utc::now();
        self.persist_manifest()
    }

    fn persist_manifest(&self) -> Result<()> {
        atomic_write_json(
            &self.data_root.join(".omnicreator/workspace.json"),
            &self.manifest,
        )
    }

    fn lease_path(&self) -> PathBuf {
        self.data_root.join(".omnicreator/writer-lease.json")
    }
}

impl WorkspaceWriter<'_> {
    pub fn workspace(&self) -> &Workspace {
        self.workspace
    }

    pub fn sqlite_path(&self) -> PathBuf {
        self.workspace.sqlite_path()
    }

    pub fn refresh_lease(&mut self) -> Result<()> {
        self.lease.updated_at = Utc::now();
        atomic_write_json(&self.workspace.lease_path(), &self.lease)
    }

    pub fn prepare_handoff(
        mut self,
        state_store: &StateStore,
        retained_snapshots: usize,
    ) -> Result<HandoffManifest> {
        self.refresh_lease()?;
        let device_id = self.lease.device_id.clone();
        let manifest = crate::handoff::create_handoff(
            self.workspace,
            state_store,
            &device_id,
            retained_snapshots,
        )?;
        self.release_inner()?;
        Ok(manifest)
    }

    pub fn release(mut self) -> Result<()> {
        self.release_inner()
    }

    fn release_inner(&mut self) -> Result<()> {
        if self.released {
            return Ok(());
        }

        let lease_path = self.workspace.lease_path();
        if lease_path.exists() {
            let current: WriterLease = serde_json::from_slice(&fs::read(&lease_path)?)?;
            if current.session_id == self.lease.session_id {
                fs::remove_file(&lease_path)?;
            }
        }
        FileExt::unlock(&self.lock_file)?;
        self.released = true;
        Ok(())
    }
}

impl Drop for WorkspaceWriter<'_> {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

fn validate_manifest(manifest: &WorkspaceManifest) -> Result<()> {
    if manifest.schema != WORKSPACE_SCHEMA {
        return Err(Error::InvalidWorkspace(format!(
            "unsupported schema {}",
            manifest.schema
        )));
    }
    if manifest.schema_version != WORKSPACE_SCHEMA_VERSION {
        return Err(Error::InvalidWorkspace(format!(
            "unsupported workspace schema version {}",
            manifest.schema_version
        )));
    }
    if !manifest.workspace_id.starts_with("ws_") {
        return Err(Error::InvalidWorkspace("invalid workspace_id".to_owned()));
    }
    Ok(())
}

fn validate_lease(lease: &WriterLease, workspace_id: &str) -> Result<()> {
    if lease.schema != WRITER_LEASE_SCHEMA
        || lease.schema_version != WRITER_LEASE_SCHEMA_VERSION
        || lease.workspace_id != workspace_id
        || lease.device_id.is_empty()
        || lease.session_id.is_empty()
    {
        return Err(Error::InvalidWorkspace(
            "writer lease is invalid for this workspace".to_owned(),
        ));
    }
    Ok(())
}

fn create_layout(root: &Path) -> Result<()> {
    for path in [
        ".omnicreator/state",
        ".omnicreator/backups",
        ".omnicreator/handoff",
        "projects",
        "library/assets",
        "studio-packs",
        "channel-profiles",
        "plugin-data",
        "exports",
        "metadata",
    ] {
        fs::create_dir_all(root.join(path))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_never_contains_absolute_data_root() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let json = serde_json::to_string(workspace.manifest()).unwrap();
        assert!(!json.contains(temp.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn second_local_writer_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        Workspace::create(&root).unwrap();

        let mut first = Workspace::open(&root).unwrap();
        let mut second = Workspace::open(&root).unwrap();

        let writer = first.acquire_writer("device-a").unwrap();
        assert!(matches!(
            second.acquire_writer("device-b"),
            Err(Error::WorkspaceBusy(_))
        ));
        writer.release().unwrap();

        let second_writer = second.acquire_writer("device-b").unwrap();
        second_writer.release().unwrap();
    }
}
