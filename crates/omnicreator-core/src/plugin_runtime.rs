use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use crate::{
    PluginManifest, PLUGIN_API_VERSION, PLUGIN_MANIFEST_SCHEMA, PLUGIN_MANIFEST_SCHEMA_VERSION,
};

pub const PLUGIN_MANIFEST_FILENAMES: [&str; 3] = ["plugin.json", "plugin.yaml", "plugin.yml"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPlugin {
    pub directory: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest: PluginManifest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PluginDiagnosticCode {
    RootReadFailed,
    ManifestNotFound,
    MultipleManifests,
    ManifestReadFailed,
    ManifestParseFailed,
    IncompatibleManifestVersion,
    IncompatibleApiVersion,
    InvalidManifest,
    DuplicatePluginId,
}

impl PluginDiagnosticCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RootReadFailed => "PLUGIN_ROOT_READ_FAILED",
            Self::ManifestNotFound => "PLUGIN_MANIFEST_NOT_FOUND",
            Self::MultipleManifests => "PLUGIN_MULTIPLE_MANIFESTS",
            Self::ManifestReadFailed => "PLUGIN_MANIFEST_READ_FAILED",
            Self::ManifestParseFailed => "PLUGIN_MANIFEST_PARSE_FAILED",
            Self::IncompatibleManifestVersion => "PLUGIN_MANIFEST_INCOMPATIBLE",
            Self::IncompatibleApiVersion => "PLUGIN_API_INCOMPATIBLE",
            Self::InvalidManifest => "PLUGIN_MANIFEST_INVALID",
            Self::DuplicatePluginId => "PLUGIN_DUPLICATE_ID",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDiagnostic {
    pub code: PluginDiagnosticCode,
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginRegistry {
    plugins: BTreeMap<String, DiscoveredPlugin>,
    by_type: BTreeMap<String, Vec<String>>,
    by_capability: BTreeMap<String, Vec<String>>,
}

impl PluginRegistry {
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    pub fn get(&self, plugin_id: &str) -> Option<&DiscoveredPlugin> {
        self.plugins.get(plugin_id)
    }

    pub fn plugins(&self) -> impl Iterator<Item = &DiscoveredPlugin> {
        self.plugins.values()
    }

    pub fn plugin_ids_for_type(&self, plugin_type: &str) -> &[String] {
        self.by_type.get(plugin_type).map_or(&[], Vec::as_slice)
    }

    pub fn plugin_ids_for_capability(&self, capability: &str) -> &[String] {
        self.by_capability
            .get(capability)
            .map_or(&[], Vec::as_slice)
    }

    fn from_plugins(plugins: BTreeMap<String, DiscoveredPlugin>) -> Self {
        let mut type_sets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut capability_sets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

        for (plugin_id, plugin) in &plugins {
            for plugin_type in &plugin.manifest.types {
                let plugin_type = plugin_type.trim();
                if !plugin_type.is_empty() {
                    type_sets
                        .entry(plugin_type.to_owned())
                        .or_default()
                        .insert(plugin_id.clone());
                }
            }
            for capability in &plugin.manifest.capabilities {
                let capability = capability.trim();
                if !capability.is_empty() {
                    capability_sets
                        .entry(capability.to_owned())
                        .or_default()
                        .insert(plugin_id.clone());
                }
            }
        }

        Self {
            plugins,
            by_type: type_sets
                .into_iter()
                .map(|(key, values)| (key, values.into_iter().collect()))
                .collect(),
            by_capability: capability_sets
                .into_iter()
                .map(|(key, values)| (key, values.into_iter().collect()))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginScanReport {
    pub registry: PluginRegistry,
    pub diagnostics: Vec<PluginDiagnostic>,
}

pub fn scan_plugin_roots(roots: &[PathBuf]) -> PluginScanReport {
    let mut roots = roots.to_vec();
    roots.sort();
    roots.dedup();

    let mut candidates: BTreeMap<String, Vec<DiscoveredPlugin>> = BTreeMap::new();
    let mut diagnostics = Vec::new();

    for root in roots {
        scan_plugin_root(&root, &mut candidates, &mut diagnostics);
    }

    let mut plugins = BTreeMap::new();
    for (plugin_id, mut matches) in candidates {
        matches.sort_by(|left, right| left.manifest_path.cmp(&right.manifest_path));
        if matches.len() == 1 {
            let plugin = matches.pop().expect("single plugin candidate");
            plugins.insert(plugin_id, plugin);
            continue;
        }

        let paths = matches
            .iter()
            .map(|plugin| plugin.manifest_path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        for plugin in matches {
            diagnostics.push(PluginDiagnostic {
                code: PluginDiagnosticCode::DuplicatePluginId,
                path: plugin.manifest_path,
                message: format!(
                    "plugin id '{plugin_id}' is declared by multiple manifests and all duplicates were rejected: {paths}"
                ),
            });
        }
    }

    diagnostics.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.message.cmp(&right.message))
    });

    PluginScanReport {
        registry: PluginRegistry::from_plugins(plugins),
        diagnostics,
    }
}

fn scan_plugin_root(
    root: &Path,
    candidates: &mut BTreeMap<String, Vec<DiscoveredPlugin>>,
    diagnostics: &mut Vec<PluginDiagnostic>,
) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            diagnostics.push(PluginDiagnostic {
                code: PluginDiagnosticCode::RootReadFailed,
                path: root.to_path_buf(),
                message: format!("cannot scan plugin root {}: {error}", root.display()),
            });
            return;
        }
    };

    let mut directories = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => match entry.file_type() {
                Ok(file_type) if file_type.is_dir() => {
                    if !entry.file_name().to_string_lossy().starts_with('.') {
                        directories.push(entry.path());
                    }
                }
                Ok(_) => {}
                Err(error) => diagnostics.push(PluginDiagnostic {
                    code: PluginDiagnosticCode::RootReadFailed,
                    path: entry.path(),
                    message: format!("cannot inspect plugin directory entry: {error}"),
                }),
            },
            Err(error) => diagnostics.push(PluginDiagnostic {
                code: PluginDiagnosticCode::RootReadFailed,
                path: root.to_path_buf(),
                message: format!("cannot read plugin directory entry: {error}"),
            }),
        }
    }
    directories.sort();

    for directory in directories {
        if let Some(plugin) = scan_plugin_directory(&directory, diagnostics) {
            candidates
                .entry(plugin.manifest.id.clone())
                .or_default()
                .push(plugin);
        }
    }
}

fn scan_plugin_directory(
    directory: &Path,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Option<DiscoveredPlugin> {
    let manifests = PLUGIN_MANIFEST_FILENAMES
        .iter()
        .map(|name| directory.join(name))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();

    if manifests.is_empty() {
        diagnostics.push(PluginDiagnostic {
            code: PluginDiagnosticCode::ManifestNotFound,
            path: directory.to_path_buf(),
            message: format!(
                "plugin directory {} does not contain any supported manifest: {}",
                directory.display(),
                PLUGIN_MANIFEST_FILENAMES.join(", ")
            ),
        });
        return None;
    }

    if manifests.len() > 1 {
        diagnostics.push(PluginDiagnostic {
            code: PluginDiagnosticCode::MultipleManifests,
            path: directory.to_path_buf(),
            message: format!(
                "plugin directory {} contains multiple manifests; keep exactly one of: {}",
                directory.display(),
                PLUGIN_MANIFEST_FILENAMES.join(", ")
            ),
        });
        return None;
    }

    let manifest_path = manifests.into_iter().next().expect("manifest exists");
    let raw = match fs::read_to_string(&manifest_path) {
        Ok(raw) => raw,
        Err(error) => {
            diagnostics.push(PluginDiagnostic {
                code: PluginDiagnosticCode::ManifestReadFailed,
                path: manifest_path,
                message: format!("cannot read plugin manifest: {error}"),
            });
            return None;
        }
    };

    let manifest = match parse_plugin_manifest(&manifest_path, &raw) {
        Ok(manifest) => manifest,
        Err(message) => {
            diagnostics.push(PluginDiagnostic {
                code: PluginDiagnosticCode::ManifestParseFailed,
                path: manifest_path,
                message,
            });
            return None;
        }
    };

    if manifest.schema != PLUGIN_MANIFEST_SCHEMA
        || manifest.schema_version != PLUGIN_MANIFEST_SCHEMA_VERSION
    {
        diagnostics.push(PluginDiagnostic {
            code: PluginDiagnosticCode::IncompatibleManifestVersion,
            path: manifest_path,
            message: format!(
                "plugin {} declares schema '{}' version {}; expected '{}' version {}",
                manifest.id,
                manifest.schema,
                manifest.schema_version,
                PLUGIN_MANIFEST_SCHEMA,
                PLUGIN_MANIFEST_SCHEMA_VERSION
            ),
        });
        return None;
    }

    if manifest.api_version != PLUGIN_API_VERSION {
        diagnostics.push(PluginDiagnostic {
            code: PluginDiagnosticCode::IncompatibleApiVersion,
            path: manifest_path,
            message: format!(
                "plugin {} declares api_version {}; expected {}",
                manifest.id, manifest.api_version, PLUGIN_API_VERSION
            ),
        });
        return None;
    }

    if let Err(error) = manifest.validate_v1() {
        diagnostics.push(PluginDiagnostic {
            code: PluginDiagnosticCode::InvalidManifest,
            path: manifest_path,
            message: error.to_string(),
        });
        return None;
    }

    Some(DiscoveredPlugin {
        directory: directory.to_path_buf(),
        manifest_path,
        manifest,
    })
}

fn parse_plugin_manifest(path: &Path, raw: &str) -> std::result::Result<PluginManifest, String> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => serde_json::from_str(raw)
            .map_err(|error| format!("invalid JSON plugin manifest: {error}")),
        Some("yaml" | "yml") => serde_yaml::from_str(raw)
            .map_err(|error| format!("invalid YAML plugin manifest: {error}")),
        _ => Err(format!(
            "unsupported plugin manifest format: {}",
            path.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    fn json_manifest(id: &str, api_version: u32, plugin_type: &str, capability: &str) -> String {
        serde_json::to_string_pretty(&json!({
            "schema": PLUGIN_MANIFEST_SCHEMA,
            "schema_version": PLUGIN_MANIFEST_SCHEMA_VERSION,
            "id": id,
            "name": format!("{id} Plugin"),
            "version": "1.0.0",
            "api_version": api_version,
            "types": [plugin_type],
            "entrypoint": {
                "command": "plugin-bin",
                "args": []
            },
            "capabilities": [capability],
            "scene_types": [],
            "permissions": {
                "filesystem": ["job-workspace"],
                "network": []
            },
            "settings": null
        }))
        .unwrap()
    }

    fn write_plugin(root: &Path, directory: &str, manifest_name: &str, content: &str) {
        let plugin_dir = root.join(directory);
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join(manifest_name), content).unwrap();
    }

    #[test]
    fn missing_plugin_root_is_an_empty_registry_without_error() {
        let temp = tempdir().unwrap();
        let report = scan_plugin_roots(&[temp.path().join("does-not-exist")]);

        assert!(report.registry.is_empty());
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn scanner_loads_json_and_yaml_and_builds_stable_indexes() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("plugins");
        fs::create_dir_all(&root).unwrap();

        write_plugin(
            &root,
            "beta",
            "plugin.yaml",
            r#"schema: omnicreator.plugin-manifest
schema_version: 1
id: beta
name: Beta Voice
version: 1.0.0
api_version: 1
types:
  - voice
entrypoint:
  command: beta-plugin
capabilities:
  - tts
permissions:
  filesystem:
    - job-workspace
  network: []
settings: null
"#,
        );
        write_plugin(
            &root,
            "alpha",
            "plugin.json",
            &json_manifest("alpha", 1, "visual", "stock_video"),
        );

        let report = scan_plugin_roots(&[root]);

        assert!(report.diagnostics.is_empty());
        assert_eq!(report.registry.len(), 2);
        assert_eq!(
            report
                .registry
                .plugins()
                .map(|plugin| plugin.manifest.id.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "beta"]
        );
        assert_eq!(
            report.registry.plugin_ids_for_type("visual"),
            &["alpha".to_owned()]
        );
        assert_eq!(
            report.registry.plugin_ids_for_capability("tts"),
            &["beta".to_owned()]
        );
    }

    #[test]
    fn incompatible_plugin_does_not_hide_valid_plugins() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("plugins");
        fs::create_dir_all(&root).unwrap();

        write_plugin(
            &root,
            "good",
            "plugin.json",
            &json_manifest("good", 1, "visual", "stock_video"),
        );
        write_plugin(
            &root,
            "future",
            "plugin.json",
            &json_manifest("future", 2, "visual", "future_capability"),
        );

        let report = scan_plugin_roots(&[root]);

        assert_eq!(report.registry.len(), 1);
        assert!(report.registry.get("good").is_some());
        assert!(report.registry.get("future").is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            PluginDiagnosticCode::IncompatibleApiVersion
        );
    }

    #[test]
    fn duplicate_plugin_ids_are_all_rejected_deterministically() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("plugins");
        fs::create_dir_all(&root).unwrap();

        write_plugin(
            &root,
            "one",
            "plugin.json",
            &json_manifest("duplicate", 1, "visual", "one"),
        );
        write_plugin(
            &root,
            "two",
            "plugin.json",
            &json_manifest("duplicate", 1, "voice", "two"),
        );

        let report = scan_plugin_roots(&[root]);

        assert!(report.registry.get("duplicate").is_none());
        assert_eq!(report.diagnostics.len(), 2);
        assert!(report
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == PluginDiagnosticCode::DuplicatePluginId));
        assert!(report.diagnostics[0].path < report.diagnostics[1].path);
    }

    #[test]
    fn multiple_manifests_in_one_plugin_directory_are_rejected() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("plugins");
        let plugin_dir = root.join("ambiguous");
        fs::create_dir_all(&plugin_dir).unwrap();
        let manifest = json_manifest("ambiguous", 1, "visual", "stock_video");
        fs::write(plugin_dir.join("plugin.json"), &manifest).unwrap();
        fs::write(
            plugin_dir.join("plugin.yaml"),
            "schema: omnicreator.plugin-manifest\n",
        )
        .unwrap();

        let report = scan_plugin_roots(&[root]);

        assert!(report.registry.is_empty());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            PluginDiagnosticCode::MultipleManifests
        );
    }

    #[test]
    fn plugin_directory_without_manifest_is_reported_not_panicked() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("plugins");
        fs::create_dir_all(root.join("broken")).unwrap();

        let report = scan_plugin_roots(&[root]);

        assert!(report.registry.is_empty());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            PluginDiagnosticCode::ManifestNotFound
        );
    }
}
