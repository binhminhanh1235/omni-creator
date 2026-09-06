use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

use serde::Serialize;
use serde_json::{json, Value};

use crate::{DiscoveredPlugin, Error, PluginManifest, Result};

pub const JOB_WORKSPACE_PERMISSION: &str = "job-workspace";
pub const PROVIDER_CACHE_PERMISSION: &str = "provider-cache";

#[derive(Debug, Clone)]
pub struct PluginJobWorkspace {
    runtime_root: PathBuf,
    jobs_root: PathBuf,
    root: PathBuf,
    input: PathBuf,
    output: PathBuf,
    temp: PathBuf,
}

#[derive(Debug, Clone)]
pub struct VerifiedPluginOutput {
    path: PathBuf,
    size_bytes: u64,
}

impl VerifiedPluginOutput {
    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginJobWorkspacePaths {
    pub root: String,
    pub input: String,
    pub output: String,
    pub temp: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginPermissionEnforcement {
    WorkspaceBound,
    DeclaredOnly,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginFilesystemPermissionReview {
    pub permission: String,
    pub allowed: bool,
    pub enforcement: PluginPermissionEnforcement,
    pub root: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginNetworkPermissionReview {
    pub target: String,
    pub enforcement: PluginPermissionEnforcement,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginPermissionReview {
    pub plugin_id: String,
    pub filesystem: Vec<PluginFilesystemPermissionReview>,
    pub network: Vec<PluginNetworkPermissionReview>,
    pub warnings: Vec<String>,
}

impl PluginJobWorkspace {
    pub fn create(runtime_root: impl AsRef<Path>, job_id: &str) -> Result<Self> {
        validate_job_id(job_id)?;

        fs::create_dir_all(runtime_root.as_ref())?;
        let runtime_root = fs::canonicalize(runtime_root.as_ref())?;
        let jobs_root = create_scoped_directory(&runtime_root, "jobs")?;
        let root = create_scoped_directory(&jobs_root, job_id)?;
        let input = create_scoped_directory(&root, "input")?;
        let output = create_scoped_directory(&root, "output")?;
        let temp = create_scoped_directory(&root, "temp")?;

        Ok(Self {
            runtime_root,
            jobs_root,
            root,
            input,
            output,
            temp,
        })
    }

    pub fn runtime_root(&self) -> &Path {
        &self.runtime_root
    }

    pub fn jobs_root(&self) -> &Path {
        &self.jobs_root
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn input_dir(&self) -> &Path {
        &self.input
    }

    pub fn output_dir(&self) -> &Path {
        &self.output
    }

    pub fn temp_dir(&self) -> &Path {
        &self.temp
    }

    pub fn provider_cache_dir(&self, plugin_id: &str) -> Result<PathBuf> {
        validate_job_id(plugin_id)?;
        let cache_root = create_scoped_directory(&self.runtime_root, "provider-cache")?;
        create_scoped_directory(&cache_root, plugin_id)
    }

    pub fn resolve_input(&self, relative: &str) -> Result<PathBuf> {
        resolve_scoped_path(&self.input, relative)
    }

    pub fn resolve_output(&self, relative: &str) -> Result<PathBuf> {
        resolve_scoped_path(&self.output, relative)
    }

    pub fn resolve_temp(&self, relative: &str) -> Result<PathBuf> {
        resolve_scoped_path(&self.temp, relative)
    }

    pub fn verify_output_file(&self, relative: &str) -> Result<VerifiedPluginOutput> {
        let candidate = self.resolve_output(relative)?;
        let metadata = fs::symlink_metadata(&candidate).map_err(|error| {
            Error::InvalidArtifact(format!(
                "plugin output is unavailable at {}: {error}",
                candidate.display()
            ))
        })?;
        if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
            return Err(Error::InvalidArtifact(format!(
                "plugin output must be a regular non-symlink file: {}",
                candidate.display()
            )));
        }

        let canonical = fs::canonicalize(&candidate)?;
        if !canonical.starts_with(&self.output) {
            return Err(Error::PathEscape(candidate.display().to_string()));
        }

        Ok(VerifiedPluginOutput {
            path: canonical,
            size_bytes: metadata.len(),
        })
    }

    pub fn protocol_paths(&self) -> Result<PluginJobWorkspacePaths> {
        Ok(PluginJobWorkspacePaths {
            root: path_utf8(&self.root)?,
            input: path_utf8(&self.input)?,
            output: path_utf8(&self.output)?,
            temp: path_utf8(&self.temp)?,
        })
    }

    pub fn initialization_context(&self, plugin: &DiscoveredPlugin) -> Result<Value> {
        let provider_cache = if plugin
            .manifest
            .permissions
            .filesystem
            .iter()
            .any(|permission| permission.trim() == PROVIDER_CACHE_PERMISSION)
        {
            Some(path_utf8(&self.provider_cache_dir(&plugin.manifest.id)?)?)
        } else {
            None
        };

        Ok(json!({
            "job_workspace": self.protocol_paths()?,
            "provider_cache": provider_cache,
            "permissions": review_plugin_permissions(&plugin.manifest, self)?,
        }))
    }
}

pub fn review_plugin_permissions(
    manifest: &PluginManifest,
    workspace: &PluginJobWorkspace,
) -> Result<PluginPermissionReview> {
    let workspace_root = path_utf8(workspace.root())?;
    let provider_cache_root = path_utf8(
        &workspace.provider_cache_dir(&manifest.id)?
    )?;
    let mut warnings = Vec::new();
    let mut filesystem = Vec::new();
    let mut seen_filesystem = BTreeSet::new();

    for raw in &manifest.permissions.filesystem {
        let permission = raw.trim();
        if permission.is_empty() {
            warnings.push("Ignored an empty filesystem permission declaration.".to_owned());
            continue;
        }
        if !seen_filesystem.insert(permission.to_owned()) {
            continue;
        }

        if permission == JOB_WORKSPACE_PERMISSION {
            filesystem.push(PluginFilesystemPermissionReview {
                permission: permission.to_owned(),
                allowed: true,
                enforcement: PluginPermissionEnforcement::WorkspaceBound,
                root: Some(workspace_root.clone()),
            });
        } else if permission == PROVIDER_CACHE_PERMISSION {
            filesystem.push(PluginFilesystemPermissionReview {
                permission: permission.to_owned(),
                allowed: true,
                enforcement: PluginPermissionEnforcement::WorkspaceBound,
                root: Some(provider_cache_root.clone()),
            });
        } else {
            filesystem.push(PluginFilesystemPermissionReview {
                permission: permission.to_owned(),
                allowed: false,
                enforcement: PluginPermissionEnforcement::Unsupported,
                root: None,
            });
            warnings.push(format!(
                "Filesystem permission '{permission}' is not supported by Plugin Runtime v1 and will not be granted."
            ));
        }
    }

    let mut network = Vec::new();
    let mut seen_network = BTreeSet::new();
    for raw in &manifest.permissions.network {
        let target = raw.trim();
        if target.is_empty() {
            warnings.push("Ignored an empty network permission declaration.".to_owned());
            continue;
        }
        if seen_network.insert(target.to_owned()) {
            network.push(PluginNetworkPermissionReview {
                target: target.to_owned(),
                enforcement: PluginPermissionEnforcement::DeclaredOnly,
            });
        }
    }
    if !network.is_empty() {
        warnings.push(
            "Network permissions are declared for review but are not OS-sandboxed by Plugin Runtime v1."
                .to_owned(),
        );
    }

    warnings.sort();
    warnings.dedup();

    Ok(PluginPermissionReview {
        plugin_id: manifest.id.clone(),
        filesystem,
        network,
        warnings,
    })
}

fn create_scoped_directory(parent: &Path, name: &str) -> Result<PathBuf> {
    let candidate = parent.join(name);
    if candidate.exists() && fs::symlink_metadata(&candidate)?.file_type().is_symlink() {
        return Err(Error::PathEscape(candidate.display().to_string()));
    }
    fs::create_dir_all(&candidate)?;
    let canonical = fs::canonicalize(&candidate)?;
    if !canonical.starts_with(parent) {
        return Err(Error::PathEscape(candidate.display().to_string()));
    }
    Ok(canonical)
}

fn resolve_scoped_path(base: &Path, relative: &str) -> Result<PathBuf> {
    validate_relative_path(relative)?;
    let candidate = base.join(relative);
    let mut current = base.to_path_buf();

    for component in Path::new(relative).components() {
        let Component::Normal(segment) = component else {
            return Err(Error::PathEscape(relative.to_owned()));
        };
        current.push(segment);
        if current.exists() && fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(Error::PathEscape(current.display().to_string()));
        }
    }

    if !candidate.starts_with(base) {
        return Err(Error::PathEscape(candidate.display().to_string()));
    }
    Ok(candidate)
}

fn validate_job_id(job_id: &str) -> Result<()> {
    if job_id.is_empty()
        || !job_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(Error::PathEscape(job_id.to_owned()));
    }
    Ok(())
}

fn validate_relative_path(relative: &str) -> Result<()> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.contains('\\')
        || relative.contains('\0')
    {
        return Err(Error::PathEscape(relative.to_owned()));
    }
    for component in Path::new(relative).components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(Error::PathEscape(relative.to_owned()));
        }
    }
    Ok(())
}

fn path_utf8(path: &Path) -> Result<String> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| Error::InvalidPathEncoding(path.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PluginEntrypoint, PluginManifest, PluginPermissions, PLUGIN_API_VERSION,
        PLUGIN_MANIFEST_SCHEMA, PLUGIN_MANIFEST_SCHEMA_VERSION,
    };

    fn manifest(filesystem: Vec<&str>, network: Vec<&str>) -> PluginManifest {
        PluginManifest {
            schema: PLUGIN_MANIFEST_SCHEMA.to_owned(),
            schema_version: PLUGIN_MANIFEST_SCHEMA_VERSION,
            id: "fixture".to_owned(),
            name: "Fixture".to_owned(),
            version: "1.0.0".to_owned(),
            api_version: PLUGIN_API_VERSION,
            types: vec!["visual".to_owned()],
            entrypoint: PluginEntrypoint {
                command: "fixture".to_owned(),
                args: Vec::new(),
            },
            capabilities: Vec::new(),
            scene_types: Vec::new(),
            permissions: PluginPermissions {
                filesystem: filesystem.into_iter().map(ToOwned::to_owned).collect(),
                network: network.into_iter().map(ToOwned::to_owned).collect(),
            },
            settings: None,
            resources: None,
        }
    }

    #[test]
    fn creates_isolated_job_layout_under_machine_runtime_root() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = PluginJobWorkspace::create(temp.path(), "job_123").unwrap();

        assert_eq!(workspace.root().parent(), Some(workspace.jobs_root()));
        assert!(workspace.input_dir().is_dir());
        assert!(workspace.output_dir().is_dir());
        assert!(workspace.temp_dir().is_dir());
        assert!(workspace.root().starts_with(workspace.runtime_root()));
    }

    #[test]
    fn rejects_job_id_and_relative_path_traversal() {
        let temp = tempfile::tempdir().unwrap();
        assert!(matches!(
            PluginJobWorkspace::create(temp.path(), "../escape"),
            Err(Error::PathEscape(_))
        ));

        let workspace = PluginJobWorkspace::create(temp.path(), "job_safe").unwrap();
        assert!(matches!(
            workspace.resolve_output("../secret.txt"),
            Err(Error::PathEscape(_))
        ));
        assert!(matches!(
            workspace.resolve_output("/absolute.txt"),
            Err(Error::PathEscape(_))
        ));
        assert!(matches!(
            workspace.resolve_output("nested\\escape.txt"),
            Err(Error::PathEscape(_))
        ));
    }

    #[test]
    fn verifies_only_regular_files_inside_output_scope() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = PluginJobWorkspace::create(temp.path(), "job_output").unwrap();
        let output = workspace.resolve_output("frames/001.png").unwrap();
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        fs::write(&output, b"frame bytes").unwrap();

        let verified = workspace.verify_output_file("frames/001.png").unwrap();
        assert_eq!(verified.size_bytes(), 11);
        assert!(verified.path().starts_with(workspace.output_dir()));

        fs::write(workspace.resolve_input("input.txt").unwrap(), b"input").unwrap();
        assert!(workspace.verify_output_file("../input/input.txt").is_err());
    }

    #[test]
    fn permission_review_grants_scoped_workspace_and_provider_cache_only() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = PluginJobWorkspace::create(temp.path(), "job_permissions").unwrap();
        let manifest = manifest(
            vec!["job-workspace", "provider-cache", "host-home", "job-workspace"],
            vec!["api.pexels.com", "api.pexels.com"],
        );

        let review = review_plugin_permissions(&manifest, &workspace).unwrap();
        assert_eq!(review.filesystem.len(), 3);
        assert!(review.filesystem[0].allowed);
        assert_eq!(
            review.filesystem[0].enforcement,
            PluginPermissionEnforcement::WorkspaceBound
        );
        assert!(review.filesystem[1].allowed);
        assert_eq!(
            review.filesystem[1].permission,
            PROVIDER_CACHE_PERMISSION
        );
        assert!(review.filesystem[1]
            .root
            .as_ref()
            .unwrap()
            .contains("provider-cache/fixture"));
        assert!(!review.filesystem[2].allowed);
        assert_eq!(
            review.filesystem[2].enforcement,
            PluginPermissionEnforcement::Unsupported
        );
        assert_eq!(review.network.len(), 1);
        assert_eq!(
            review.network[0].enforcement,
            PluginPermissionEnforcement::DeclaredOnly
        );
        assert!(review
            .warnings
            .iter()
            .any(|warning| warning.contains("not OS-sandboxed")));
    }

    #[test]
    fn initialization_context_contains_runtime_paths_and_permission_review() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = PluginJobWorkspace::create(temp.path(), "job_context").unwrap();
        let plugin = DiscoveredPlugin {
            directory: temp.path().join("plugin"),
            manifest_path: temp.path().join("plugin/plugin.json"),
            manifest: manifest(vec!["job-workspace", "provider-cache"], Vec::new()),
        };

        let context = workspace.initialization_context(&plugin).unwrap();
        assert_eq!(context["permissions"]["plugin_id"], "fixture");
        assert!(context["job_workspace"]["output"]
            .as_str()
            .unwrap()
            .contains("job_context"));
        assert!(context["provider_cache"]
            .as_str()
            .unwrap()
            .contains("provider-cache/fixture"));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_inside_output_scope_cannot_escape() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), b"secret").unwrap();

        let workspace =
            PluginJobWorkspace::create(temp.path().join("runtime"), "job_link").unwrap();
        symlink(&outside, workspace.output_dir().join("escape")).unwrap();

        assert!(matches!(
            workspace.resolve_output("escape/secret.txt"),
            Err(Error::PathEscape(_))
        ));
    }
}
