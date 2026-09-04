use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Error, Result};

pub const WORKSPACE_SCHEMA: &str = "omnicreator.workspace";
pub const WORKSPACE_SCHEMA_VERSION: u32 = 1;

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

#[derive(Debug, Clone)]
pub struct Workspace {
    data_root: PathBuf,
    manifest: WorkspaceManifest,
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

    pub fn mark_writer_open(&mut self, device_id: &str) -> Result<()> {
        self.manifest.last_clean_shutdown = false;
        self.manifest.last_writer_device = Some(device_id.to_owned());
        self.manifest.updated_at = Utc::now();
        self.persist_manifest()
    }

    pub fn mark_clean_shutdown(&mut self, device_id: &str) -> Result<()> {
        self.manifest.revision += 1;
        self.manifest.last_clean_shutdown = true;
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

fn atomic_write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::InvalidWorkspace("manifest has no parent".to_owned()))?;
    fs::create_dir_all(parent)?;

    let temp = parent.join(format!(".workspace-{}.tmp", Uuid::new_v4().simple()));
    let bytes = serde_json::to_vec_pretty(value)?;
    {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    fs::rename(&temp, path)?;
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
}
