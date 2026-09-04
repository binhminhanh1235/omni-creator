use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{fs_util::atomic_write_json, Error, Result, Workspace};

pub const MACHINE_BINDING_SCHEMA: &str = "omnicreator.machine-binding";
pub const MACHINE_BINDING_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MachineBinding {
    pub schema: String,
    pub schema_version: u32,
    pub workspace_id: String,
    pub data_root: PathBuf,
    pub device_id: String,
}

impl MachineBinding {
    pub fn for_workspace(workspace: &Workspace, device_id: impl Into<String>) -> Self {
        Self {
            schema: MACHINE_BINDING_SCHEMA.to_owned(),
            schema_version: MACHINE_BINDING_SCHEMA_VERSION,
            workspace_id: workspace.manifest().workspace_id.clone(),
            data_root: workspace.data_root().to_path_buf(),
            device_id: device_id.into(),
        }
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        validate_binding(self)?;
        atomic_write_json(path.as_ref(), self)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let binding: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        validate_binding(&binding)?;
        Ok(binding)
    }

    pub fn open_workspace(&self) -> Result<Workspace> {
        let workspace = Workspace::open(&self.data_root)?;
        if workspace.manifest().workspace_id != self.workspace_id {
            return Err(Error::WorkspaceBindingMismatch {
                expected: self.workspace_id.clone(),
                actual: workspace.manifest().workspace_id.clone(),
            });
        }
        Ok(workspace)
    }
}

fn validate_binding(binding: &MachineBinding) -> Result<()> {
    if binding.schema != MACHINE_BINDING_SCHEMA {
        return Err(Error::InvalidMachineBinding(format!(
            "unsupported schema {}",
            binding.schema
        )));
    }
    if binding.schema_version != MACHINE_BINDING_SCHEMA_VERSION {
        return Err(Error::InvalidMachineBinding(format!(
            "unsupported schema version {}",
            binding.schema_version
        )));
    }
    if binding.workspace_id.is_empty() || binding.device_id.is_empty() {
        return Err(Error::InvalidMachineBinding(
            "workspace_id and device_id are required".to_owned(),
        ));
    }
    if !binding.data_root.is_absolute() {
        return Err(Error::InvalidMachineBinding(
            "data_root must be an absolute machine-local path".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_binding_is_saved_outside_portable_workspace_and_reopens_it() {
        let temp = tempfile::tempdir().unwrap();
        let data_root = temp.path().join("portable");
        let local_config = temp.path().join("machine/config.json");

        let workspace = Workspace::create(&data_root).unwrap();
        let binding = MachineBinding::for_workspace(&workspace, "device-a");
        binding.save(&local_config).unwrap();

        let loaded = MachineBinding::load(&local_config).unwrap();
        let reopened = loaded.open_workspace().unwrap();
        assert_eq!(
            reopened.manifest().workspace_id,
            workspace.manifest().workspace_id
        );
        assert!(!data_root.join("machine/config.json").exists());
    }
}
