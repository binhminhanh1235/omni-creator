use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{scan_plugin_roots, Error, PluginDiagnostic, PluginRegistry, Result};

pub const PLUGIN_LIFECYCLE_SCHEMA_V1: &str = "omnicreator.plugin-lifecycle";
pub const PLUGIN_LIFECYCLE_SCHEMA_VERSION_V1: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginInstallSourceV1 {
    BuiltIn,
    UserInstalled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginTrustV1 {
    BuiltIn,
    LocalUnverified,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginCompatibilityV1 {
    Compatible,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginLifecycleStateV1 {
    pub schema: String,
    pub schema_version: u32,
    #[serde(default)]
    pub disabled_plugin_ids: BTreeSet<String>,
}

impl Default for PluginLifecycleStateV1 {
    fn default() -> Self {
        Self {
            schema: PLUGIN_LIFECYCLE_SCHEMA_V1.to_owned(),
            schema_version: PLUGIN_LIFECYCLE_SCHEMA_VERSION_V1,
            disabled_plugin_ids: BTreeSet::new(),
        }
    }
}

impl PluginLifecycleStateV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != PLUGIN_LIFECYCLE_SCHEMA_V1
            || self.schema_version != PLUGIN_LIFECYCLE_SCHEMA_VERSION_V1
        {
            return Err(Error::InvalidContract(format!(
                "unsupported plugin lifecycle schema '{}' version {}; expected '{}' version {}",
                self.schema,
                self.schema_version,
                PLUGIN_LIFECYCLE_SCHEMA_V1,
                PLUGIN_LIFECYCLE_SCHEMA_VERSION_V1
            )));
        }

        if let Some(plugin_id) = self
            .disabled_plugin_ids
            .iter()
            .find(|plugin_id| plugin_id.trim().is_empty() || plugin_id.trim() != plugin_id.as_str())
        {
            return Err(Error::InvalidContract(format!(
                "invalid disabled plugin id '{plugin_id}'"
            )));
        }
        Ok(())
    }

    pub fn load_v1(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let state: Self = serde_json::from_slice(&fs::read(path)?)?;
        state.validate_v1()?;
        Ok(state)
    }

    pub fn save_v1(&self, path: impl AsRef<Path>) -> Result<()> {
        self.validate_v1()?;
        let path = path.as_ref();
        let parent = path.parent().ok_or_else(|| {
            Error::InvalidContract("plugin lifecycle state path has no parent".to_owned())
        })?;
        fs::create_dir_all(parent)?;
        let temporary = parent.join(format!(
            ".{}.tmp",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("plugin-lifecycle.json")
        ));
        fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temporary, path)?;
        Ok(())
    }

    pub fn is_enabled_v1(&self, plugin_id: &str) -> bool {
        !self.disabled_plugin_ids.contains(plugin_id)
    }

    pub fn set_enabled_v1(&mut self, plugin_id: &str, enabled: bool) -> Result<()> {
        let plugin_id = plugin_id.trim();
        if plugin_id.is_empty() {
            return Err(Error::InvalidContract(
                "plugin id must not be empty".to_owned(),
            ));
        }
        if enabled {
            self.disabled_plugin_ids.remove(plugin_id);
        } else {
            self.disabled_plugin_ids.insert(plugin_id.to_owned());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginInventoryEntryV1 {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
    pub types: Vec<String>,
    pub capabilities: Vec<String>,
    pub enabled: bool,
    pub source: PluginInstallSourceV1,
    pub trust: PluginTrustV1,
    pub compatibility: PluginCompatibilityV1,
}

#[derive(Debug, Clone)]
pub struct PluginInventoryReportV1 {
    pub registry: PluginRegistry,
    pub inventory: Vec<PluginInventoryEntryV1>,
    pub diagnostics: Vec<PluginDiagnostic>,
}

pub fn scan_plugin_inventory_v1(
    built_in_roots: &[PathBuf],
    user_installed_roots: &[PathBuf],
    lifecycle: &PluginLifecycleStateV1,
) -> PluginInventoryReportV1 {
    let built_in_roots = normalized_roots_v1(built_in_roots);
    let user_installed_roots = normalized_roots_v1(user_installed_roots);
    let mut all_roots = built_in_roots.clone();
    all_roots.extend(user_installed_roots.clone());
    all_roots.sort();
    all_roots.dedup();

    let scan = scan_plugin_roots(&all_roots);
    let mut inventory = scan
        .registry
        .plugins()
        .map(|plugin| {
            let user_installed = user_installed_roots
                .iter()
                .any(|root| plugin.directory.starts_with(root));
            let source = if user_installed {
                PluginInstallSourceV1::UserInstalled
            } else {
                PluginInstallSourceV1::BuiltIn
            };
            let trust = if user_installed {
                PluginTrustV1::LocalUnverified
            } else {
                PluginTrustV1::BuiltIn
            };
            let mut types = plugin.manifest.types.clone();
            types.sort();
            types.dedup();
            let mut capabilities = plugin.manifest.capabilities.clone();
            capabilities.sort();
            capabilities.dedup();

            PluginInventoryEntryV1 {
                id: plugin.manifest.id.clone(),
                name: plugin.manifest.name.clone(),
                version: plugin.manifest.version.clone(),
                api_version: plugin.manifest.api_version,
                types,
                capabilities,
                enabled: lifecycle.is_enabled_v1(&plugin.manifest.id),
                source,
                trust,
                compatibility: PluginCompatibilityV1::Compatible,
            }
        })
        .collect::<Vec<_>>();
    inventory.sort_by(|left, right| left.id.cmp(&right.id));

    PluginInventoryReportV1 {
        registry: scan.registry,
        inventory,
        diagnostics: scan.diagnostics,
    }
}

fn normalized_roots_v1(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut normalized = roots
        .iter()
        .map(|root| fs::canonicalize(root).unwrap_or_else(|_| root.clone()))
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::{PLUGIN_API_VERSION, PLUGIN_MANIFEST_SCHEMA, PLUGIN_MANIFEST_SCHEMA_VERSION};

    fn write_plugin(root: &Path, directory: &str, id: &str, api_version: u32, capability: &str) {
        let plugin_dir = root.join(directory);
        fs::create_dir_all(&plugin_dir).unwrap();
        let manifest = json!({
            "schema": PLUGIN_MANIFEST_SCHEMA,
            "schema_version": PLUGIN_MANIFEST_SCHEMA_VERSION,
            "id": id,
            "name": format!("{id} Plugin"),
            "version": "1.2.3",
            "api_version": api_version,
            "types": ["visual"],
            "entrypoint": {"command": "plugin-bin", "args": []},
            "capabilities": [capability],
            "scene_types": [],
            "permissions": {"filesystem": ["job-workspace"], "network": []},
            "settings": null
        });
        fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn lifecycle_state_round_trips_deterministically_outside_portable_state() {
        let temp = tempdir().unwrap();
        let machine_config = temp.path().join("machine-config/plugin-lifecycle.json");
        let mut state = PluginLifecycleStateV1::default();
        state.set_enabled_v1("pexels", false).unwrap();
        state
            .set_enabled_v1("generated-image-reference", false)
            .unwrap();
        state.save_v1(&machine_config).unwrap();

        let loaded = PluginLifecycleStateV1::load_v1(&machine_config).unwrap();
        assert_eq!(loaded, state);
        assert!(!loaded.is_enabled_v1("pexels"));
        assert!(loaded.is_enabled_v1("unsplash"));

        let encoded = fs::read_to_string(&machine_config).unwrap();
        assert!(encoded.contains(PLUGIN_LIFECYCLE_SCHEMA_V1));
        assert!(!encoded.contains("Data Root"));
    }

    #[test]
    fn inventory_classifies_roots_and_keeps_disabled_plugin_discoverable() {
        let temp = tempdir().unwrap();
        let built_in = temp.path().join("built-in");
        let user = temp.path().join("user");
        fs::create_dir_all(&built_in).unwrap();
        fs::create_dir_all(&user).unwrap();
        write_plugin(
            &built_in,
            "pexels",
            "pexels",
            PLUGIN_API_VERSION,
            "stock_video",
        );
        write_plugin(
            &user,
            "custom-visual",
            "custom-visual",
            PLUGIN_API_VERSION,
            "generated_still",
        );

        let mut lifecycle = PluginLifecycleStateV1::default();
        lifecycle.set_enabled_v1("custom-visual", false).unwrap();
        let report = scan_plugin_inventory_v1(&[built_in], &[user], &lifecycle);

        assert!(report.diagnostics.is_empty());
        assert_eq!(report.registry.len(), 2);
        assert!(report.registry.get("custom-visual").is_some());
        assert_eq!(report.inventory.len(), 2);

        let built_in_entry = report
            .inventory
            .iter()
            .find(|entry| entry.id == "pexels")
            .unwrap();
        assert_eq!(built_in_entry.source, PluginInstallSourceV1::BuiltIn);
        assert_eq!(built_in_entry.trust, PluginTrustV1::BuiltIn);
        assert!(built_in_entry.enabled);

        let user_entry = report
            .inventory
            .iter()
            .find(|entry| entry.id == "custom-visual")
            .unwrap();
        assert_eq!(user_entry.source, PluginInstallSourceV1::UserInstalled);
        assert_eq!(user_entry.trust, PluginTrustV1::LocalUnverified);
        assert_eq!(user_entry.compatibility, PluginCompatibilityV1::Compatible);
        assert!(!user_entry.enabled);
        assert_eq!(user_entry.api_version, PLUGIN_API_VERSION);
        assert_eq!(user_entry.version, "1.2.3");
    }

    #[test]
    fn duplicate_ids_across_sources_use_canonical_registry_rejection() {
        let temp = tempdir().unwrap();
        let built_in = temp.path().join("built-in");
        let user = temp.path().join("user");
        fs::create_dir_all(&built_in).unwrap();
        fs::create_dir_all(&user).unwrap();
        write_plugin(
            &built_in,
            "one",
            "same-id",
            PLUGIN_API_VERSION,
            "stock_video",
        );
        write_plugin(
            &user,
            "two",
            "same-id",
            PLUGIN_API_VERSION,
            "generated_still",
        );

        let report =
            scan_plugin_inventory_v1(&[built_in], &[user], &PluginLifecycleStateV1::default());

        assert!(report.registry.get("same-id").is_none());
        assert!(report.inventory.is_empty());
        assert_eq!(report.diagnostics.len(), 2);
        assert!(report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code.as_str() == "PLUGIN_DUPLICATE_ID"));
    }

    #[test]
    fn incompatible_api_stays_diagnostic_not_active_inventory() {
        let temp = tempdir().unwrap();
        let user = temp.path().join("user");
        fs::create_dir_all(&user).unwrap();
        write_plugin(
            &user,
            "future",
            "future",
            PLUGIN_API_VERSION + 1,
            "future_capability",
        );

        let report = scan_plugin_inventory_v1(&[], &[user], &PluginLifecycleStateV1::default());

        assert!(report.inventory.is_empty());
        assert!(report.registry.is_empty());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            "PLUGIN_API_INCOMPATIBLE"
        );
    }

    #[test]
    fn invalid_lifecycle_schema_is_rejected() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("plugin-lifecycle.json");
        fs::write(
            &path,
            br#"{"schema":"omnicreator.plugin-lifecycle","schema_version":2,"disabled_plugin_ids":[]}"#,
        )
        .unwrap();

        assert!(matches!(
            PluginLifecycleStateV1::load_v1(path),
            Err(Error::InvalidContract(_))
        ));
    }
}
