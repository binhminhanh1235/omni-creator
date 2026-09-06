use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{
    scan_plugin_roots, Error, PluginDiagnostic, PluginManifest, PluginRegistry,
    PortableStudioPackCatalogV1, Project, Result, StudioPackRouteTargetV1,
};

pub const PLUGIN_LIFECYCLE_SCHEMA_V1: &str = "omnicreator.plugin-lifecycle";
pub const PLUGIN_LIFECYCLE_SCHEMA_VERSION_V1: u32 = 1;

static PLUGIN_STAGING_SEQUENCE_V1: AtomicU64 = AtomicU64::new(0);

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInstallOutcomeV1 {
    pub plugin_id: String,
    pub version: String,
    pub install_directory: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginUninstallOutcomeV1 {
    pub plugin_id: String,
    pub removed_directory: PathBuf,
}

pub fn install_local_plugin_folder_v1(
    source_directory: impl AsRef<Path>,
    built_in_roots: &[PathBuf],
    user_plugin_root: impl AsRef<Path>,
) -> Result<PluginInstallOutcomeV1> {
    let source_directory = source_directory.as_ref();
    let source_metadata = fs::symlink_metadata(source_directory)?;
    if source_metadata.file_type().is_symlink() || !source_metadata.is_dir() {
        return Err(Error::InvalidContract(format!(
            "local plugin source must be a real directory, not a symlink or file: {}",
            source_directory.display()
        )));
    }

    fs::create_dir_all(user_plugin_root.as_ref())?;
    let user_plugin_root = fs::canonicalize(user_plugin_root.as_ref())?;
    let source_directory = fs::canonicalize(source_directory)?;
    if source_directory.starts_with(&user_plugin_root) {
        return Err(Error::InvalidContract(format!(
            "local plugin source must be outside the managed user plugin root: {}",
            user_plugin_root.display()
        )));
    }

    let staging_root = user_plugin_root.join(".install-staging");
    fs::create_dir_all(&staging_root)?;
    let session = create_plugin_staging_session_v1(&staging_root, "install")?;
    let staged_directory = session.join("candidate");

    let result = (|| {
        copy_plugin_directory_v1(&source_directory, &staged_directory)?;

        let staged_scan = scan_plugin_roots(std::slice::from_ref(&session));
        if !staged_scan.diagnostics.is_empty() || staged_scan.registry.len() != 1 {
            return Err(Error::InvalidContract(format!(
                "local plugin package failed canonical manifest validation: {}",
                plugin_diagnostics_summary_v1(&staged_scan.diagnostics)
            )));
        }

        let staged_plugin = staged_scan
            .registry
            .plugins()
            .next()
            .expect("one staged plugin was just counted");
        let plugin_id = staged_plugin.manifest.id.clone();
        let version = staged_plugin.manifest.version.clone();

        let mut combined_roots = built_in_roots.to_vec();
        combined_roots.push(user_plugin_root.clone());
        combined_roots.push(session.clone());
        let combined_scan = scan_plugin_roots(&combined_roots);
        let combined_plugin = combined_scan.registry.get(&plugin_id).ok_or_else(|| {
            Error::InvalidContract(format!(
                "plugin id '{plugin_id}' is already installed or collides with an existing plugin"
            ))
        })?;

        let combined_directory = fs::canonicalize(&combined_plugin.directory)?;
        let staged_directory_canonical = fs::canonicalize(&staged_directory)?;
        if combined_directory != staged_directory_canonical {
            return Err(Error::InvalidContract(format!(
                "plugin id '{plugin_id}' is already installed on this machine"
            )));
        }

        let install_directory = user_plugin_root.join(plugin_install_directory_name_v1(&plugin_id));
        if install_directory.exists() {
            return Err(Error::InvalidContract(format!(
                "plugin id '{plugin_id}' already has a managed installation directory"
            )));
        }

        fs::rename(&staged_directory, &install_directory)?;
        Ok(PluginInstallOutcomeV1 {
            plugin_id,
            version,
            install_directory,
        })
    })();

    let _ = fs::remove_dir_all(&session);
    let _ = fs::remove_dir(&staging_root);
    result
}

pub fn uninstall_user_plugin_v1(
    plugin_id: &str,
    built_in_roots: &[PathBuf],
    user_plugin_root: impl AsRef<Path>,
) -> Result<PluginUninstallOutcomeV1> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() {
        return Err(Error::InvalidContract(
            "plugin id must not be empty for uninstall".to_owned(),
        ));
    }

    let user_plugin_root = user_plugin_root.as_ref();
    let user_scan = scan_plugin_roots(&[user_plugin_root.to_path_buf()]);
    let Some(user_plugin) = user_scan.registry.get(plugin_id) else {
        let built_in_scan = scan_plugin_roots(built_in_roots);
        if built_in_scan.registry.get(plugin_id).is_some() {
            return Err(Error::InvalidContract(format!(
                "built-in plugin '{plugin_id}' cannot be uninstalled"
            )));
        }
        return Err(Error::InvalidContract(format!(
            "user-installed plugin '{plugin_id}' was not found"
        )));
    };

    let user_plugin_root = fs::canonicalize(user_plugin_root)?;
    let plugin_directory = fs::canonicalize(&user_plugin.directory)?;
    if plugin_directory.parent() != Some(user_plugin_root.as_path()) {
        return Err(Error::InvalidContract(format!(
            "refusing to uninstall plugin '{plugin_id}' outside the managed user plugin root"
        )));
    }

    let staging_root = user_plugin_root.join(".uninstall-staging");
    fs::create_dir_all(&staging_root)?;
    let session = create_plugin_staging_session_v1(&staging_root, "uninstall")?;
    let tombstone = session.join("plugin");
    fs::rename(&plugin_directory, &tombstone)?;

    if let Err(remove_error) = fs::remove_dir_all(&tombstone) {
        let rollback = fs::rename(&tombstone, &plugin_directory);
        let rollback_note = match rollback {
            Ok(()) => "installation restored".to_owned(),
            Err(error) => format!("rollback also failed: {error}"),
        };
        let _ = fs::remove_dir_all(&session);
        let _ = fs::remove_dir(&staging_root);
        return Err(Error::InvalidContract(format!(
            "failed to remove user plugin '{plugin_id}': {remove_error}; {rollback_note}"
        )));
    }

    let _ = fs::remove_dir_all(&session);
    let _ = fs::remove_dir(&staging_root);
    Ok(PluginUninstallOutcomeV1 {
        plugin_id: plugin_id.to_owned(),
        removed_directory: plugin_directory,
    })
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginCapabilityDeltaV1 {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub retained: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginUpdatePreviewV1 {
    pub plugin_id: String,
    pub installed_version: String,
    pub candidate_version: String,
    pub installed_types: Vec<String>,
    pub candidate_types: Vec<String>,
    pub installed_capabilities: Vec<String>,
    pub candidate_capabilities: Vec<String>,
    pub capability_delta: PluginCapabilityDeltaV1,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginUpdateOutcomeV1 {
    pub preview: PluginUpdatePreviewV1,
    pub install_directory: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginMutationKindV1 {
    Disable,
    Remove,
    Update,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginCapabilityImpactV1 {
    pub plugin_id: String,
    pub mutation: PluginMutationKindV1,
    pub lost_capabilities: Vec<String>,
    pub gained_capabilities: Vec<String>,
    pub affected_pack_ids: Vec<String>,
    pub blocking_pack_ids: Vec<String>,
    pub affected_project_ids: Vec<String>,
    pub blocking_project_ids: Vec<String>,
}

struct StagedPluginCandidateV1 {
    session: PathBuf,
    directory: PathBuf,
    manifest: PluginManifest,
}

pub fn inspect_local_plugin_update_v1(
    expected_plugin_id: &str,
    source_directory: impl AsRef<Path>,
    built_in_roots: &[PathBuf],
    user_plugin_root: impl AsRef<Path>,
) -> Result<PluginUpdatePreviewV1> {
    let user_plugin_root = user_plugin_root.as_ref();
    let staged = stage_local_plugin_candidate_v1(
        source_directory.as_ref(),
        user_plugin_root,
        "update-inspect",
    )?;
    let result = inspect_staged_plugin_update_v1(
        expected_plugin_id,
        &staged.manifest,
        built_in_roots,
        user_plugin_root,
    );
    cleanup_plugin_staging_v1(&staged.session, user_plugin_root.join(".update-staging"));
    result
}

pub fn update_local_plugin_folder_v1(
    expected_plugin_id: &str,
    source_directory: impl AsRef<Path>,
    built_in_roots: &[PathBuf],
    user_plugin_root: impl AsRef<Path>,
) -> Result<PluginUpdateOutcomeV1> {
    let user_plugin_root = user_plugin_root.as_ref();
    let staged =
        stage_local_plugin_candidate_v1(source_directory.as_ref(), user_plugin_root, "update")?;
    let staging_root = user_plugin_root.join(".update-staging");

    let result = (|| {
        let preview = inspect_staged_plugin_update_v1(
            expected_plugin_id,
            &staged.manifest,
            built_in_roots,
            user_plugin_root,
        )?;

        let canonical_user_root = fs::canonicalize(user_plugin_root)?;
        let user_scan = scan_plugin_roots(std::slice::from_ref(&canonical_user_root));
        let current = user_scan.registry.get(expected_plugin_id).ok_or_else(|| {
            Error::InvalidContract(format!(
                "user-installed plugin '{expected_plugin_id}' was not found for update"
            ))
        })?;
        let current_directory = fs::canonicalize(&current.directory)?;
        if current_directory.parent() != Some(canonical_user_root.as_path()) {
            return Err(Error::InvalidContract(format!(
                "refusing to update plugin '{expected_plugin_id}' outside the managed user plugin root"
            )));
        }

        let candidate_version = preview.candidate_version.clone();
        let built_in_roots = built_in_roots.to_vec();
        let verification_root = canonical_user_root.clone();
        activate_plugin_update_v1(
            &current_directory,
            &staged.directory,
            &staged.session,
            || {
                let mut roots = built_in_roots.clone();
                roots.push(verification_root.clone());
                let scan = scan_plugin_roots(&roots);
                let plugin = scan.registry.get(expected_plugin_id).ok_or_else(|| {
                    Error::InvalidContract(format!(
                        "updated plugin '{expected_plugin_id}' did not pass post-activation discovery"
                    ))
                })?;
                if plugin.manifest.version != candidate_version {
                    return Err(Error::InvalidContract(format!(
                        "updated plugin '{expected_plugin_id}' version mismatch after activation"
                    )));
                }
                Ok(())
            },
        )?;

        Ok(PluginUpdateOutcomeV1 {
            preview,
            install_directory: current_directory,
        })
    })();

    cleanup_plugin_staging_v1(&staged.session, staging_root);
    result
}

pub fn preview_plugin_capability_impact_v1(
    registry: &PluginRegistry,
    lifecycle: &PluginLifecycleStateV1,
    catalog: &PortableStudioPackCatalogV1,
    projects: &[Project],
    plugin_id: &str,
    mutation: PluginMutationKindV1,
    update: Option<&PluginUpdatePreviewV1>,
) -> Result<PluginCapabilityImpactV1> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() {
        return Err(Error::InvalidContract(
            "plugin id must not be empty for impact preview".to_owned(),
        ));
    }
    let current = registry.get(plugin_id).ok_or_else(|| {
        Error::InvalidContract(format!(
            "plugin '{plugin_id}' is not installed for impact preview"
        ))
    })?;

    let replacement = match mutation {
        PluginMutationKindV1::Update => {
            let update = update.ok_or_else(|| {
                Error::InvalidContract(
                    "update impact preview requires an inspected update candidate".to_owned(),
                )
            })?;
            if update.plugin_id != plugin_id {
                return Err(Error::InvalidContract(format!(
                    "update preview plugin id '{}' does not match selected plugin '{plugin_id}'",
                    update.plugin_id
                )));
            }
            Some((
                update.candidate_types.as_slice(),
                update.candidate_capabilities.as_slice(),
            ))
        }
        PluginMutationKindV1::Disable | PluginMutationKindV1::Remove => None,
    };

    let current_capabilities = normalized_strings_v1(&current.manifest.capabilities);
    let candidate_capabilities = replacement
        .map(|(_, capabilities)| normalized_strings_v1(capabilities))
        .unwrap_or_default();
    let all_capabilities = current_capabilities
        .iter()
        .chain(candidate_capabilities.iter())
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut lost_capabilities = Vec::new();
    let mut gained_capabilities = Vec::new();
    for capability in all_capabilities {
        let before = enabled_provider_exists_for_capability_v1(
            registry,
            lifecycle,
            &capability,
            plugin_id,
            None,
            false,
        );
        let after = enabled_provider_exists_for_capability_v1(
            registry,
            lifecycle,
            &capability,
            plugin_id,
            replacement,
            mutation != PluginMutationKindV1::Update,
        );
        if before && !after {
            lost_capabilities.push(capability.clone());
        } else if !before && after {
            gained_capabilities.push(capability.clone());
        }
    }

    let mut affected_pack_ids = Vec::new();
    let mut blocking_pack_ids = Vec::new();
    for definition in catalog.list_definitions_v1()? {
        let effective = catalog.resolve_v1(&definition.id)?;
        let mut affected = false;
        let mut blocking = false;

        for route in effective.config.routes.values() {
            let before = route
                .targets
                .iter()
                .map(|target| {
                    target_has_enabled_provider_v1(
                        registry, lifecycle, target, plugin_id, None, false,
                    )
                })
                .collect::<Vec<_>>();
            let after = route
                .targets
                .iter()
                .map(|target| {
                    target_has_enabled_provider_v1(
                        registry,
                        lifecycle,
                        target,
                        plugin_id,
                        replacement,
                        mutation != PluginMutationKindV1::Update,
                    )
                })
                .collect::<Vec<_>>();

            if before != after {
                affected = true;
            }
            if before.iter().any(|available| *available)
                && !after.iter().any(|available| *available)
            {
                blocking = true;
            }
        }

        if affected {
            affected_pack_ids.push(definition.id.clone());
        }
        if blocking {
            blocking_pack_ids.push(definition.id.clone());
        }
    }

    affected_pack_ids.sort();
    affected_pack_ids.dedup();
    blocking_pack_ids.sort();
    blocking_pack_ids.dedup();

    let affected_pack_set = affected_pack_ids.iter().collect::<BTreeSet<_>>();
    let blocking_pack_set = blocking_pack_ids.iter().collect::<BTreeSet<_>>();
    let mut affected_project_ids = projects
        .iter()
        .filter(|project| {
            project
                .studio_pack
                .as_ref()
                .is_some_and(|pack_id| affected_pack_set.contains(pack_id))
        })
        .map(|project| project.id.clone())
        .collect::<Vec<_>>();
    let mut blocking_project_ids = projects
        .iter()
        .filter(|project| {
            project
                .studio_pack
                .as_ref()
                .is_some_and(|pack_id| blocking_pack_set.contains(pack_id))
        })
        .map(|project| project.id.clone())
        .collect::<Vec<_>>();
    affected_project_ids.sort();
    affected_project_ids.dedup();
    blocking_project_ids.sort();
    blocking_project_ids.dedup();

    Ok(PluginCapabilityImpactV1 {
        plugin_id: plugin_id.to_owned(),
        mutation,
        lost_capabilities,
        gained_capabilities,
        affected_pack_ids,
        blocking_pack_ids,
        affected_project_ids,
        blocking_project_ids,
    })
}

fn stage_local_plugin_candidate_v1(
    source_directory: &Path,
    user_plugin_root: &Path,
    operation: &str,
) -> Result<StagedPluginCandidateV1> {
    let source_metadata = fs::symlink_metadata(source_directory)?;
    if source_metadata.file_type().is_symlink() || !source_metadata.is_dir() {
        return Err(Error::InvalidContract(format!(
            "local plugin update source must be a real directory, not a symlink or file: {}",
            source_directory.display()
        )));
    }

    fs::create_dir_all(user_plugin_root)?;
    let canonical_user_root = fs::canonicalize(user_plugin_root)?;
    let source_directory = fs::canonicalize(source_directory)?;
    if source_directory.starts_with(&canonical_user_root) {
        return Err(Error::InvalidContract(format!(
            "local plugin update source must be outside the managed user plugin root: {}",
            canonical_user_root.display()
        )));
    }

    let staging_root = canonical_user_root.join(".update-staging");
    fs::create_dir_all(&staging_root)?;
    let session = create_plugin_staging_session_v1(&staging_root, operation)?;
    let directory = session.join("candidate");

    let result = (|| {
        copy_plugin_directory_v1(&source_directory, &directory)?;
        let scan = scan_plugin_roots(std::slice::from_ref(&session));
        if !scan.diagnostics.is_empty() || scan.registry.len() != 1 {
            return Err(Error::InvalidContract(format!(
                "local plugin update candidate failed canonical manifest validation: {}",
                plugin_diagnostics_summary_v1(&scan.diagnostics)
            )));
        }
        let manifest = scan
            .registry
            .plugins()
            .next()
            .expect("one staged update candidate was just counted")
            .manifest
            .clone();
        Ok(StagedPluginCandidateV1 {
            session: session.clone(),
            directory,
            manifest,
        })
    })();

    if result.is_err() {
        cleanup_plugin_staging_v1(&session, staging_root);
    }
    result
}

fn inspect_staged_plugin_update_v1(
    expected_plugin_id: &str,
    candidate: &PluginManifest,
    built_in_roots: &[PathBuf],
    user_plugin_root: &Path,
) -> Result<PluginUpdatePreviewV1> {
    let expected_plugin_id = expected_plugin_id.trim();
    if expected_plugin_id.is_empty() {
        return Err(Error::InvalidContract(
            "expected plugin id must not be empty for update".to_owned(),
        ));
    }
    if candidate.id != expected_plugin_id {
        return Err(Error::InvalidContract(format!(
            "update candidate plugin id '{}' does not match installed plugin '{expected_plugin_id}'",
            candidate.id
        )));
    }

    let built_in = scan_plugin_roots(built_in_roots);
    if built_in.registry.get(expected_plugin_id).is_some() {
        return Err(Error::InvalidContract(format!(
            "built-in plugin '{expected_plugin_id}' cannot be updated from a local package"
        )));
    }

    let user_scan = scan_plugin_roots(&[user_plugin_root.to_path_buf()]);
    let installed = user_scan.registry.get(expected_plugin_id).ok_or_else(|| {
        Error::InvalidContract(format!(
            "user-installed plugin '{expected_plugin_id}' was not found for update"
        ))
    })?;

    let installed_version = Version::parse(&installed.manifest.version).map_err(|error| {
        Error::InvalidContract(format!(
            "installed plugin '{expected_plugin_id}' version '{}' is not valid SemVer: {error}",
            installed.manifest.version
        ))
    })?;
    let candidate_version = Version::parse(&candidate.version).map_err(|error| {
        Error::InvalidContract(format!(
            "update candidate for '{expected_plugin_id}' version '{}' is not valid SemVer: {error}",
            candidate.version
        ))
    })?;
    if candidate_version <= installed_version {
        return Err(Error::InvalidContract(format!(
            "update candidate for '{expected_plugin_id}' must be newer than installed version {}; found {}",
            installed.manifest.version, candidate.version
        )));
    }

    let installed_types = normalized_strings_v1(&installed.manifest.types);
    let candidate_types = normalized_strings_v1(&candidate.types);
    let installed_capabilities = normalized_strings_v1(&installed.manifest.capabilities);
    let candidate_capabilities = normalized_strings_v1(&candidate.capabilities);
    let installed_set = installed_capabilities
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let candidate_set = candidate_capabilities
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();

    Ok(PluginUpdatePreviewV1 {
        plugin_id: expected_plugin_id.to_owned(),
        installed_version: installed.manifest.version.clone(),
        candidate_version: candidate.version.clone(),
        installed_types,
        candidate_types,
        installed_capabilities,
        candidate_capabilities,
        capability_delta: PluginCapabilityDeltaV1 {
            added: candidate_set.difference(&installed_set).cloned().collect(),
            removed: installed_set.difference(&candidate_set).cloned().collect(),
            retained: installed_set
                .intersection(&candidate_set)
                .cloned()
                .collect(),
        },
    })
}

fn activate_plugin_update_v1<F>(
    current_directory: &Path,
    candidate_directory: &Path,
    session: &Path,
    verify: F,
) -> Result<()>
where
    F: FnOnce() -> Result<()>,
{
    let previous = session.join("previous");
    fs::rename(current_directory, &previous)?;

    if let Err(error) = fs::rename(candidate_directory, current_directory) {
        let rollback = fs::rename(&previous, current_directory);
        let note = match rollback {
            Ok(()) => "previous installation restored".to_owned(),
            Err(rollback_error) => format!("rollback also failed: {rollback_error}"),
        };
        return Err(Error::InvalidContract(format!(
            "failed to activate plugin update: {error}; {note}"
        )));
    }

    if let Err(error) = verify() {
        let failed_candidate = session.join("failed-candidate");
        let _ = fs::rename(current_directory, &failed_candidate);
        let rollback = fs::rename(&previous, current_directory);
        let note = match rollback {
            Ok(()) => "previous installation restored".to_owned(),
            Err(rollback_error) => format!("rollback also failed: {rollback_error}"),
        };
        let _ = fs::remove_dir_all(&failed_candidate);
        return Err(Error::InvalidContract(format!(
            "plugin update failed post-activation verification: {error}; {note}"
        )));
    }

    let _ = fs::remove_dir_all(&previous);
    Ok(())
}

fn cleanup_plugin_staging_v1(session: &Path, staging_root: PathBuf) {
    let _ = fs::remove_dir_all(session);
    let _ = fs::remove_dir(staging_root);
}

fn normalized_strings_v1(values: &[String]) -> Vec<String> {
    let mut values = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn enabled_provider_exists_for_capability_v1(
    registry: &PluginRegistry,
    lifecycle: &PluginLifecycleStateV1,
    capability: &str,
    mutated_plugin_id: &str,
    replacement: Option<(&[String], &[String])>,
    remove_mutated: bool,
) -> bool {
    registry.plugins().any(|plugin| {
        if !lifecycle.is_enabled_v1(&plugin.manifest.id) {
            return false;
        }
        if plugin.manifest.id == mutated_plugin_id {
            if remove_mutated {
                return false;
            }
            let capabilities = replacement
                .map(|(_, capabilities)| capabilities)
                .unwrap_or(plugin.manifest.capabilities.as_slice());
            return capabilities.iter().any(|value| value == capability);
        }
        plugin
            .manifest
            .capabilities
            .iter()
            .any(|value| value == capability)
    })
}

fn target_has_enabled_provider_v1(
    registry: &PluginRegistry,
    lifecycle: &PluginLifecycleStateV1,
    target: &StudioPackRouteTargetV1,
    mutated_plugin_id: &str,
    replacement: Option<(&[String], &[String])>,
    remove_mutated: bool,
) -> bool {
    registry.plugins().any(|plugin| {
        if !lifecycle.is_enabled_v1(&plugin.manifest.id) {
            return false;
        }

        if plugin.manifest.id == mutated_plugin_id {
            if remove_mutated {
                return false;
            }
            let (types, capabilities) = replacement.unwrap_or((
                plugin.manifest.types.as_slice(),
                plugin.manifest.capabilities.as_slice(),
            ));
            return target_matches_contract_v1(target, &plugin.manifest.id, types, capabilities);
        }

        target_matches_contract_v1(
            target,
            &plugin.manifest.id,
            &plugin.manifest.types,
            &plugin.manifest.capabilities,
        )
    })
}

fn target_matches_contract_v1(
    target: &StudioPackRouteTargetV1,
    plugin_id: &str,
    types: &[String],
    capabilities: &[String],
) -> bool {
    let id_match = target
        .plugin_id
        .as_deref()
        .is_none_or(|required| required == plugin_id);
    id_match
        && types.iter().any(|value| value == &target.plugin_type)
        && capabilities.iter().any(|value| value == &target.capability)
}

fn create_plugin_staging_session_v1(root: &Path, operation: &str) -> Result<PathBuf> {
    loop {
        let sequence = PLUGIN_STAGING_SEQUENCE_V1.fetch_add(1, Ordering::Relaxed);
        let session = root.join(format!("{operation}-{}-{sequence}", std::process::id()));
        match fs::create_dir(&session) {
            Ok(()) => return Ok(session),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

fn plugin_install_directory_name_v1(plugin_id: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(7 + plugin_id.len() * 2);
    encoded.push_str("plugin-");
    for byte in plugin_id.as_bytes() {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn copy_plugin_directory_v1(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return Err(Error::InvalidContract(format!(
            "plugin package contains a symlink and cannot be installed safely: {}",
            source.display()
        )));
    }
    if !metadata.is_dir() {
        return Err(Error::InvalidContract(format!(
            "plugin package entry is not a directory: {}",
            source.display()
        )));
    }

    fs::create_dir(destination)?;
    let mut entries = fs::read_dir(source)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(Error::InvalidContract(format!(
                "plugin package contains a symlink and cannot be installed safely: {}",
                source_path.display()
            )));
        }
        if file_type.is_dir() {
            copy_plugin_directory_v1(&source_path, &destination_path)?;
            continue;
        }
        if file_type.is_file() {
            let source_metadata = fs::metadata(&source_path)?;
            fs::copy(&source_path, &destination_path)?;
            fs::set_permissions(&destination_path, source_metadata.permissions())?;
            continue;
        }
        return Err(Error::InvalidContract(format!(
            "plugin package contains an unsupported filesystem entry: {}",
            source_path.display()
        )));
    }
    Ok(())
}

fn plugin_diagnostics_summary_v1(diagnostics: &[PluginDiagnostic]) -> String {
    if diagnostics.is_empty() {
        return "expected exactly one valid plugin manifest".to_owned();
    }
    diagnostics
        .iter()
        .map(|diagnostic| {
            format!(
                "{} at {}: {}",
                diagnostic.code.as_str(),
                diagnostic.path.display(),
                diagnostic.message
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
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
    fn valid_local_plugin_folder_installs_without_executing_entrypoint() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let user_root = temp.path().join("machine-plugins");
        fs::create_dir_all(&source_root).unwrap();
        write_plugin(
            &source_root,
            "package",
            "local-safe",
            PLUGIN_API_VERSION,
            "generated_still",
        );
        let source = source_root.join("package");
        let marker = temp.path().join("entrypoint-ran");
        fs::write(
            source.join("plugin-bin"),
            format!("#!/bin/sh\ntouch {}\n", marker.display()),
        )
        .unwrap();
        fs::create_dir_all(source.join("resources")).unwrap();
        fs::write(source.join("resources/model.txt"), b"portable resource").unwrap();

        let outcome = install_local_plugin_folder_v1(&source, &[], &user_root).unwrap();

        assert_eq!(outcome.plugin_id, "local-safe");
        assert_eq!(outcome.version, "1.2.3");
        assert!(outcome.install_directory.starts_with(&user_root));
        assert!(outcome.install_directory.join("plugin.json").is_file());
        assert!(outcome
            .install_directory
            .join("resources/model.txt")
            .is_file());
        assert!(!marker.exists());
        assert!(!user_root.join(".install-staging").exists());

        let scan = scan_plugin_roots(&[user_root]);
        assert!(scan.diagnostics.is_empty());
        assert!(scan.registry.get("local-safe").is_some());
    }

    #[test]
    fn invalid_or_incompatible_package_never_activates() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let user_root = temp.path().join("machine-plugins");
        fs::create_dir_all(&source_root).unwrap();
        write_plugin(
            &source_root,
            "future",
            "future-local",
            PLUGIN_API_VERSION + 1,
            "future_capability",
        );

        let result = install_local_plugin_folder_v1(source_root.join("future"), &[], &user_root);

        assert!(matches!(result, Err(Error::InvalidContract(_))));
        assert!(scan_plugin_roots(std::slice::from_ref(&user_root))
            .registry
            .is_empty());
        assert!(!user_root.join(".install-staging").exists());
    }

    #[test]
    fn existing_plugin_id_is_not_overwritten() {
        let temp = tempdir().unwrap();
        let built_in = temp.path().join("built-in");
        let source_root = temp.path().join("source");
        let user_root = temp.path().join("machine-plugins");
        fs::create_dir_all(&built_in).unwrap();
        fs::create_dir_all(&source_root).unwrap();
        write_plugin(
            &built_in,
            "shipped",
            "same-id",
            PLUGIN_API_VERSION,
            "stock_video",
        );
        write_plugin(
            &source_root,
            "candidate",
            "same-id",
            PLUGIN_API_VERSION,
            "generated_still",
        );

        let result = install_local_plugin_folder_v1(
            source_root.join("candidate"),
            std::slice::from_ref(&built_in),
            &user_root,
        );

        assert!(matches!(result, Err(Error::InvalidContract(_))));
        assert!(scan_plugin_roots(std::slice::from_ref(&user_root))
            .registry
            .is_empty());
        assert!(scan_plugin_roots(&[built_in])
            .registry
            .get("same-id")
            .is_some());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_in_local_package_is_rejected_and_staging_is_cleaned() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let user_root = temp.path().join("machine-plugins");
        fs::create_dir_all(&source_root).unwrap();
        write_plugin(
            &source_root,
            "candidate",
            "symlinked",
            PLUGIN_API_VERSION,
            "generated_still",
        );
        let outside = temp.path().join("outside.txt");
        fs::write(&outside, b"outside").unwrap();
        symlink(
            &outside,
            source_root.join("candidate").join("linked-secret.txt"),
        )
        .unwrap();

        let result = install_local_plugin_folder_v1(source_root.join("candidate"), &[], &user_root);

        assert!(matches!(result, Err(Error::InvalidContract(_))));
        assert!(outside.exists());
        assert!(scan_plugin_roots(std::slice::from_ref(&user_root))
            .registry
            .is_empty());
        assert!(!user_root.join(".install-staging").exists());
    }

    #[test]
    fn uninstall_removes_only_user_installation_and_leaves_portable_state_untouched() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let user_root = temp.path().join("machine-plugins");
        let portable_root = temp.path().join("data-root");
        fs::create_dir_all(&source_root).unwrap();
        fs::create_dir_all(portable_root.join("projects")).unwrap();
        let project_state = portable_root.join("projects/project.json");
        fs::write(&project_state, br#"{"id":"project-1"}"#).unwrap();
        write_plugin(
            &source_root,
            "candidate",
            "remove-me",
            PLUGIN_API_VERSION,
            "generated_still",
        );

        let installed =
            install_local_plugin_folder_v1(source_root.join("candidate"), &[], &user_root).unwrap();
        assert!(installed.install_directory.exists());

        let removed = uninstall_user_plugin_v1("remove-me", &[], &user_root).unwrap();

        assert_eq!(removed.plugin_id, "remove-me");
        assert!(!removed.removed_directory.exists());
        assert_eq!(
            fs::read_to_string(project_state).unwrap(),
            r#"{"id":"project-1"}"#
        );
        assert!(scan_plugin_roots(std::slice::from_ref(&user_root))
            .registry
            .is_empty());
        assert!(!user_root.join(".uninstall-staging").exists());
    }

    #[test]
    fn built_in_plugin_cannot_be_uninstalled() {
        let temp = tempdir().unwrap();
        let built_in = temp.path().join("built-in");
        let user_root = temp.path().join("machine-plugins");
        fs::create_dir_all(&built_in).unwrap();
        fs::create_dir_all(&user_root).unwrap();
        write_plugin(
            &built_in,
            "shipped",
            "shipped-plugin",
            PLUGIN_API_VERSION,
            "stock_video",
        );

        let result = uninstall_user_plugin_v1(
            "shipped-plugin",
            std::slice::from_ref(&built_in),
            &user_root,
        );

        assert!(matches!(result, Err(Error::InvalidContract(_))));
        assert!(scan_plugin_roots(&[built_in])
            .registry
            .get("shipped-plugin")
            .is_some());
    }

    fn write_plugin_contract_v1(
        root: &Path,
        directory: &str,
        id: &str,
        version: &str,
        api_version: u32,
        plugin_type: &str,
        capabilities: &[&str],
    ) {
        let plugin_dir = root.join(directory);
        fs::create_dir_all(&plugin_dir).unwrap();
        let manifest = json!({
            "schema": PLUGIN_MANIFEST_SCHEMA,
            "schema_version": PLUGIN_MANIFEST_SCHEMA_VERSION,
            "id": id,
            "name": format!("{id} Plugin"),
            "version": version,
            "api_version": api_version,
            "types": [plugin_type],
            "entrypoint": {"command": "plugin-bin", "args": []},
            "capabilities": capabilities,
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
    fn update_inspection_reports_semver_and_capability_delta() {
        let temp = tempdir().unwrap();
        let user_root = temp.path().join("user");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&user_root).unwrap();
        fs::create_dir_all(&source_root).unwrap();
        write_plugin_contract_v1(
            &user_root,
            "current",
            "local-visual",
            "1.0.0",
            PLUGIN_API_VERSION,
            "visual",
            &["old_capability", "shared_capability"],
        );
        write_plugin_contract_v1(
            &source_root,
            "candidate",
            "local-visual",
            "1.1.0",
            PLUGIN_API_VERSION,
            "visual",
            &["shared_capability", "new_capability"],
        );

        let preview = inspect_local_plugin_update_v1(
            "local-visual",
            source_root.join("candidate"),
            &[],
            &user_root,
        )
        .unwrap();

        assert_eq!(preview.installed_version, "1.0.0");
        assert_eq!(preview.candidate_version, "1.1.0");
        assert_eq!(preview.capability_delta.added, vec!["new_capability"]);
        assert_eq!(preview.capability_delta.removed, vec!["old_capability"]);
        assert_eq!(preview.capability_delta.retained, vec!["shared_capability"]);
        assert!(!user_root.join(".update-staging").exists());
    }

    #[test]
    fn update_candidate_must_match_id_and_be_strictly_newer_semver() {
        let temp = tempdir().unwrap();
        let user_root = temp.path().join("user");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&user_root).unwrap();
        fs::create_dir_all(&source_root).unwrap();
        write_plugin_contract_v1(
            &user_root,
            "current",
            "local-visual",
            "1.0.0",
            PLUGIN_API_VERSION,
            "visual",
            &["generated_still"],
        );
        write_plugin_contract_v1(
            &source_root,
            "wrong-id",
            "different-plugin",
            "2.0.0",
            PLUGIN_API_VERSION,
            "visual",
            &["generated_still"],
        );
        write_plugin_contract_v1(
            &source_root,
            "same-version",
            "local-visual",
            "1.0.0",
            PLUGIN_API_VERSION,
            "visual",
            &["generated_still"],
        );
        write_plugin_contract_v1(
            &source_root,
            "bad-version",
            "local-visual",
            "nightly",
            PLUGIN_API_VERSION,
            "visual",
            &["generated_still"],
        );

        assert!(inspect_local_plugin_update_v1(
            "local-visual",
            source_root.join("wrong-id"),
            &[],
            &user_root,
        )
        .is_err());
        assert!(inspect_local_plugin_update_v1(
            "local-visual",
            source_root.join("same-version"),
            &[],
            &user_root,
        )
        .is_err());
        assert!(inspect_local_plugin_update_v1(
            "local-visual",
            source_root.join("bad-version"),
            &[],
            &user_root,
        )
        .is_err());
    }

    #[test]
    fn incompatible_api_update_is_rejected_before_activation() {
        let temp = tempdir().unwrap();
        let user_root = temp.path().join("user");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&user_root).unwrap();
        fs::create_dir_all(&source_root).unwrap();
        write_plugin_contract_v1(
            &user_root,
            "current",
            "local-visual",
            "1.0.0",
            PLUGIN_API_VERSION,
            "visual",
            &["generated_still"],
        );
        write_plugin_contract_v1(
            &source_root,
            "candidate",
            "local-visual",
            "2.0.0",
            PLUGIN_API_VERSION + 1,
            "visual",
            &["generated_still"],
        );

        let result = update_local_plugin_folder_v1(
            "local-visual",
            source_root.join("candidate"),
            &[],
            &user_root,
        );

        assert!(matches!(result, Err(Error::InvalidContract(_))));
        let current = scan_plugin_roots(std::slice::from_ref(&user_root));
        assert_eq!(
            current
                .registry
                .get("local-visual")
                .unwrap()
                .manifest
                .version,
            "1.0.0"
        );
    }

    #[test]
    fn user_plugin_update_atomically_replaces_installed_contract() {
        let temp = tempdir().unwrap();
        let user_root = temp.path().join("user");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&user_root).unwrap();
        fs::create_dir_all(&source_root).unwrap();
        write_plugin_contract_v1(
            &user_root,
            "current",
            "local-visual",
            "1.0.0",
            PLUGIN_API_VERSION,
            "visual",
            &["old_capability"],
        );
        fs::write(user_root.join("current/old-only.txt"), b"old").unwrap();
        write_plugin_contract_v1(
            &source_root,
            "candidate",
            "local-visual",
            "1.2.0",
            PLUGIN_API_VERSION,
            "visual",
            &["new_capability"],
        );
        fs::write(source_root.join("candidate/new-only.txt"), b"new").unwrap();

        let outcome = update_local_plugin_folder_v1(
            "local-visual",
            source_root.join("candidate"),
            &[],
            &user_root,
        )
        .unwrap();

        assert_eq!(outcome.preview.installed_version, "1.0.0");
        assert_eq!(outcome.preview.candidate_version, "1.2.0");
        assert!(outcome.install_directory.join("new-only.txt").is_file());
        assert!(!outcome.install_directory.join("old-only.txt").exists());
        let scan = scan_plugin_roots(std::slice::from_ref(&user_root));
        let plugin = scan.registry.get("local-visual").unwrap();
        assert_eq!(plugin.manifest.version, "1.2.0");
        assert_eq!(plugin.manifest.capabilities, vec!["new_capability"]);
        assert!(!user_root.join(".update-staging").exists());
    }

    #[test]
    fn failed_post_activation_verification_restores_previous_plugin() {
        let temp = tempdir().unwrap();
        let current = temp.path().join("current");
        let session = temp.path().join("session");
        let candidate = session.join("candidate");
        fs::create_dir_all(&current).unwrap();
        fs::create_dir_all(&candidate).unwrap();
        fs::write(current.join("version.txt"), b"old").unwrap();
        fs::write(candidate.join("version.txt"), b"new").unwrap();

        let result = activate_plugin_update_v1(&current, &candidate, &session, || {
            Err(Error::InvalidContract(
                "forced post-activation verification failure".to_owned(),
            ))
        });

        assert!(matches!(result, Err(Error::InvalidContract(_))));
        assert_eq!(fs::read(current.join("version.txt")).unwrap(), b"old");
        assert!(!candidate.exists());
    }

    #[test]
    fn built_in_plugin_local_update_is_rejected() {
        let temp = tempdir().unwrap();
        let built_in = temp.path().join("built-in");
        let user_root = temp.path().join("user");
        let source_root = temp.path().join("source");
        fs::create_dir_all(&built_in).unwrap();
        fs::create_dir_all(&user_root).unwrap();
        fs::create_dir_all(&source_root).unwrap();
        write_plugin_contract_v1(
            &built_in,
            "shipped",
            "shipped-plugin",
            "1.0.0",
            PLUGIN_API_VERSION,
            "visual",
            &["stock_video"],
        );
        write_plugin_contract_v1(
            &source_root,
            "candidate",
            "shipped-plugin",
            "2.0.0",
            PLUGIN_API_VERSION,
            "visual",
            &["stock_video"],
        );

        let result = inspect_local_plugin_update_v1(
            "shipped-plugin",
            source_root.join("candidate"),
            std::slice::from_ref(&built_in),
            &user_root,
        );

        assert!(matches!(result, Err(Error::InvalidContract(_))));
        assert!(scan_plugin_roots(std::slice::from_ref(&built_in))
            .registry
            .get("shipped-plugin")
            .is_some());
    }

    #[test]
    fn capability_impact_projects_studio_pack_and_project_blockers_without_mutation() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("plugins");
        fs::create_dir_all(&root).unwrap();
        write_plugin_contract_v1(
            &root,
            "pexels",
            "pexels",
            "1.0.0",
            PLUGIN_API_VERSION,
            "visual",
            &["stock_video", "stock_image"],
        );
        write_plugin_contract_v1(
            &root,
            "generated",
            "generated",
            "1.0.0",
            PLUGIN_API_VERSION,
            "visual",
            &["generated_still"],
        );
        write_plugin_contract_v1(
            &root,
            "stick",
            "stick",
            "1.0.0",
            PLUGIN_API_VERSION,
            "visual",
            &["stick_figure_visual"],
        );
        let registry = scan_plugin_roots(std::slice::from_ref(&root)).registry;
        let catalog = crate::initial_studio_pack_catalog_v1().unwrap();
        let now = chrono::Utc::now();
        let projects = vec![Project {
            id: "project-stick".to_owned(),
            title: "Stick Project".to_owned(),
            created_at: now,
            updated_at: now,
            studio_pack: Some("christian-stick-explainer".to_owned()),
            channel_profile: None,
            script_version: 1,
            production_lock: false,
        }];
        let catalog_before = catalog.canonical_json_v1().unwrap();
        let projects_before = projects.clone();

        let impact = preview_plugin_capability_impact_v1(
            &registry,
            &PluginLifecycleStateV1::default(),
            &catalog,
            &projects,
            "stick",
            PluginMutationKindV1::Remove,
            None,
        )
        .unwrap();

        assert_eq!(impact.lost_capabilities, vec!["stick_figure_visual"]);
        assert!(impact
            .affected_pack_ids
            .contains(&"christian-stick-explainer".to_owned()));
        assert!(impact
            .blocking_pack_ids
            .contains(&"christian-stick-explainer".to_owned()));
        assert_eq!(impact.affected_project_ids, vec!["project-stick"]);
        assert_eq!(impact.blocking_project_ids, vec!["project-stick"]);
        assert_eq!(catalog.canonical_json_v1().unwrap(), catalog_before);
        assert_eq!(projects, projects_before);
    }

    #[test]
    fn update_impact_uses_candidate_contract_without_activating_it() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("plugins");
        fs::create_dir_all(&root).unwrap();
        write_plugin_contract_v1(
            &root,
            "visual",
            "visual",
            "1.0.0",
            PLUGIN_API_VERSION,
            "visual",
            &["generated_still"],
        );
        let registry = scan_plugin_roots(std::slice::from_ref(&root)).registry;
        let preview = PluginUpdatePreviewV1 {
            plugin_id: "visual".to_owned(),
            installed_version: "1.0.0".to_owned(),
            candidate_version: "2.0.0".to_owned(),
            installed_types: vec!["visual".to_owned()],
            candidate_types: vec!["visual".to_owned()],
            installed_capabilities: vec!["generated_still".to_owned()],
            candidate_capabilities: vec!["stock_image".to_owned()],
            capability_delta: PluginCapabilityDeltaV1 {
                added: vec!["stock_image".to_owned()],
                removed: vec!["generated_still".to_owned()],
                retained: vec![],
            },
        };

        let impact = preview_plugin_capability_impact_v1(
            &registry,
            &PluginLifecycleStateV1::default(),
            &crate::initial_studio_pack_catalog_v1().unwrap(),
            &[],
            "visual",
            PluginMutationKindV1::Update,
            Some(&preview),
        )
        .unwrap();

        assert_eq!(impact.lost_capabilities, vec!["generated_still"]);
        assert_eq!(impact.gained_capabilities, vec!["stock_image"]);
        assert_eq!(registry.get("visual").unwrap().manifest.version, "1.0.0");
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
