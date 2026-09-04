use std::{fs, fs::File, path::PathBuf};

use fs2::FileExt;
use uuid::Uuid;

use crate::{
    fs_util::atomic_write_json,
    handoff::HandoffManifest,
    workspace::{validate_lease, WriterLease, WRITER_LEASE_SCHEMA, WRITER_LEASE_SCHEMA_VERSION},
    Error, Result, StateStore, Workspace,
};

pub struct WorkspaceSession {
    workspace: Workspace,
    lock_file: File,
    lease: WriterLease,
    released: bool,
}

impl WorkspaceSession {
    pub fn acquire(mut workspace: Workspace, device_id: &str) -> Result<Self> {
        if device_id.trim().is_empty() {
            return Err(Error::InvalidWorkspace(
                "device_id is required for writer acquisition".to_owned(),
            ));
        }

        let lock_path = workspace.data_root().join(".omnicreator/writer.lock");
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;

        if FileExt::try_lock_exclusive(&lock_file).is_err() {
            return Err(Error::WorkspaceBusy(
                "another local process already owns the workspace lock".to_owned(),
            ));
        }

        let lease_path = workspace.lease_path();
        let result = (|| {
            if lease_path.exists() {
                let existing: WriterLease = serde_json::from_slice(&fs::read(&lease_path)?)?;
                validate_lease(&existing, &workspace.manifest().workspace_id)?;
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
                workspace_id: workspace.manifest().workspace_id.clone(),
                device_id: device_id.to_owned(),
                session_id: format!("session_{}", Uuid::new_v4().simple()),
                updated_at: chrono::Utc::now(),
            };
            atomic_write_json(&lease_path, &lease)?;
            workspace.mark_writer_open(device_id)?;
            Ok(lease)
        })();

        match result {
            Ok(lease) => Ok(Self {
                workspace,
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

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn sqlite_path(&self) -> PathBuf {
        self.workspace.sqlite_path()
    }

    pub fn device_id(&self) -> &str {
        &self.lease.device_id
    }

    pub fn refresh_lease(&mut self) -> Result<()> {
        self.lease.updated_at = chrono::Utc::now();
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
            &mut self.workspace,
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

impl Drop for WorkspaceSession {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_session_holds_writer_lock_until_released() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        Workspace::create(&root).unwrap();

        let session =
            WorkspaceSession::acquire(Workspace::open(&root).unwrap(), "device-a").unwrap();

        assert!(matches!(
            WorkspaceSession::acquire(Workspace::open(&root).unwrap(), "device-b"),
            Err(Error::WorkspaceBusy(_))
        ));

        session.release().unwrap();

        let second =
            WorkspaceSession::acquire(Workspace::open(&root).unwrap(), "device-b").unwrap();
        second.release().unwrap();
    }
}
