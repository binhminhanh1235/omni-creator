use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::Utc;
use uuid::Uuid;

use crate::{
    fs_util::sha256_file, Artifact, Error, LogicalUri, PathResolver, PluginJobWorkspace, Result,
    StateStore,
};

#[derive(Debug, Clone)]
pub struct PluginOutputPromotion {
    pub relative_output: String,
    pub target_uri: LogicalUri,
    pub artifact_type: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ArtifactStore {
    resolver: PathResolver,
}

impl ArtifactStore {
    pub fn new(data_root: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            resolver: PathResolver::new(data_root)?,
        })
    }

    pub fn data_root(&self) -> &Path {
        self.resolver.data_root()
    }

    pub fn promote_job_output(
        &self,
        state_store: &mut StateStore,
        job_id: &str,
        source: impl AsRef<Path>,
        target_uri: LogicalUri,
        artifact_type: impl Into<String>,
        metadata: serde_json::Value,
    ) -> Result<Artifact> {
        let source = source.as_ref();
        if !source.is_file() {
            return Err(Error::InvalidArtifact(format!(
                "source file does not exist: {}",
                source.display()
            )));
        }

        let artifact_type = artifact_type.into();
        if artifact_type.trim().is_empty() {
            return Err(Error::InvalidArtifact(
                "artifact_type must not be empty".to_owned(),
            ));
        }

        if matches!(&target_uri, LogicalUri::Artifact(_)) {
            return Err(Error::InvalidArtifact(
                "artifact:// cannot be used as a physical promotion target".to_owned(),
            ));
        }

        let job = state_store.get_job(job_id)?;
        let project_context = match &target_uri {
            LogicalUri::Project(_) => Some(job.project_id.as_str()),
            _ => None,
        };

        let mut destination = self.resolver.resolve(&target_uri, project_context)?;
        if destination.exists() {
            return Err(Error::ArtifactTargetExists(destination));
        }

        let parent = destination
            .parent()
            .ok_or_else(|| Error::InvalidArtifact("target has no parent directory".to_owned()))?;
        fs::create_dir_all(parent)?;

        destination = self.resolver.resolve(&target_uri, project_context)?;
        if destination.exists() {
            return Err(Error::ArtifactTargetExists(destination));
        }

        let parent = destination
            .parent()
            .ok_or_else(|| Error::InvalidArtifact("target has no parent directory".to_owned()))?;
        let temp = parent.join(format!(".artifact-{}.tmp", Uuid::new_v4().simple()));

        if let Err(error) = copy_and_sync(source, &temp) {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }

        let (sha256, size_bytes) = sha256_file(&temp)?;
        if let Err(error) = fs::rename(&temp, &destination) {
            let _ = fs::remove_file(&temp);
            return Err(error.into());
        }

        let artifact = Artifact {
            artifact_id: format!("art_{}", Uuid::new_v4().simple()),
            project_id: Some(job.project_id.clone()),
            artifact_type,
            uri: target_uri,
            sha256,
            size_bytes,
            input_hash: Some(job.input_hash.clone()),
            producer_job: Some(job.job_id.clone()),
            created_at: Utc::now(),
            metadata,
        };

        if let Err(error) = state_store.commit_job_success(&artifact) {
            let _ = fs::remove_file(&destination);
            return Err(error);
        }

        Ok(artifact)
    }

    pub fn promote_plugin_output(
        &self,
        state_store: &mut StateStore,
        job_id: &str,
        workspace: &PluginJobWorkspace,
        promotion: PluginOutputPromotion,
    ) -> Result<Artifact> {
        let verified = workspace.verify_output_file(&promotion.relative_output)?;
        self.promote_job_output(
            state_store,
            job_id,
            verified.path(),
            promotion.target_uri,
            promotion.artifact_type,
            promotion.metadata,
        )
    }

    pub fn lookup_verified_cache(
        &self,
        state_store: &StateStore,
        input_hash: &str,
    ) -> Result<Option<Artifact>> {
        let Some(artifact) = state_store.find_cached_artifact(input_hash)? else {
            return Ok(None);
        };

        if !self.verify_artifact(&artifact)? {
            return Ok(None);
        }
        Ok(Some(artifact))
    }

    pub fn verify_artifact(&self, artifact: &Artifact) -> Result<bool> {
        let project_context = match &artifact.uri {
            LogicalUri::Project(_) => artifact.project_id.as_deref(),
            _ => None,
        };
        let path = self.resolver.resolve(&artifact.uri, project_context)?;
        if !path.exists() {
            return Ok(false);
        }

        let (sha256, size_bytes) = sha256_file(&path)?;
        if sha256 != artifact.sha256 || size_bytes != artifact.size_bytes {
            return Err(Error::ArtifactHashMismatch(artifact.artifact_id.clone()));
        }
        Ok(true)
    }

    pub fn resolve_artifact_path(&self, artifact: &Artifact) -> Result<PathBuf> {
        let project_context = match artifact.uri {
            LogicalUri::Project(_) => artifact.project_id.as_deref(),
            _ => None,
        };
        self.resolver.resolve(&artifact.uri, project_context)
    }
}

fn copy_and_sync(source: &Path, destination: &Path) -> Result<()> {
    fs::copy(source, destination)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(destination)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{deterministic_input_hash, StepStatus, Workspace};

    #[test]
    fn promotion_records_verified_artifact_and_cache_survives_restart() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        let workspace = Workspace::create(&root).unwrap();
        let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = state.create_project("Artifact Project").unwrap();

        let input_hash = deterministic_input_hash(&[b"omnivoice-v3", b"hello world"]);
        let job = state
            .create_job(&project.id, "tts", "S01", &input_hash)
            .unwrap();

        let source = temp.path().join("generated.wav");
        fs::write(&source, b"fake but deterministic audio bytes").unwrap();

        let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
        let artifact = artifacts
            .promote_job_output(
                &mut state,
                &job.job_id,
                &source,
                LogicalUri::parse("project://audio/S01.wav").unwrap(),
                "audio",
                serde_json::json!({"provider": "test"}),
            )
            .unwrap();

        let persisted_job = state.get_job(&job.job_id).unwrap();
        assert_eq!(persisted_job.status, StepStatus::Succeeded);
        assert_eq!(
            persisted_job.selected_artifact.as_deref(),
            Some(artifact.artifact_id.as_str())
        );
        assert!(!artifact
            .uri
            .as_str()
            .contains(root.to_string_lossy().as_ref()));

        let cached = artifacts
            .lookup_verified_cache(&state, &input_hash)
            .unwrap()
            .unwrap();
        assert_eq!(cached.artifact_id, artifact.artifact_id);

        drop(state);
        let reopened = StateStore::open(workspace.sqlite_path()).unwrap();
        let cached_after_restart = artifacts
            .lookup_verified_cache(&reopened, &input_hash)
            .unwrap()
            .unwrap();
        assert_eq!(cached_after_restart.artifact_id, artifact.artifact_id);
    }

    #[test]
    fn plugin_output_is_verified_inside_job_workspace_before_promotion() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        let workspace = Workspace::create(&root).unwrap();
        let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = state.create_project("Plugin Artifact Project").unwrap();
        let input_hash = deterministic_input_hash(&[b"plugin", b"scene"]);
        let job = state
            .create_job(&project.id, "visual", "SC01", &input_hash)
            .unwrap();

        let plugin_workspace =
            PluginJobWorkspace::create(temp.path().join("runtime"), &job.job_id).unwrap();
        let output = plugin_workspace.resolve_output("scene/frame.png").unwrap();
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        fs::write(&output, b"verified plugin frame").unwrap();

        let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
        let artifact = artifacts
            .promote_plugin_output(
                &mut state,
                &job.job_id,
                &plugin_workspace,
                PluginOutputPromotion {
                    relative_output: "scene/frame.png".to_owned(),
                    target_uri: LogicalUri::parse("project://visual/SC01.png").unwrap(),
                    artifact_type: "image".to_owned(),
                    metadata: serde_json::json!({"provider": "fixture-plugin"}),
                },
            )
            .unwrap();

        assert_eq!(
            state.get_job(&job.job_id).unwrap().status,
            StepStatus::Succeeded
        );
        assert!(artifacts.verify_artifact(&artifact).unwrap());
    }

    #[test]
    fn plugin_output_traversal_is_rejected_before_artifact_promotion() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        let workspace = Workspace::create(&root).unwrap();
        let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = state.create_project("Plugin Traversal Project").unwrap();
        let input_hash = deterministic_input_hash(&[b"plugin", b"escape"]);
        let job = state
            .create_job(&project.id, "visual", "SC02", &input_hash)
            .unwrap();

        let plugin_workspace =
            PluginJobWorkspace::create(temp.path().join("runtime"), &job.job_id).unwrap();
        let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();

        assert!(matches!(
            artifacts.promote_plugin_output(
                &mut state,
                &job.job_id,
                &plugin_workspace,
                PluginOutputPromotion {
                    relative_output: "../outside.png".to_owned(),
                    target_uri: LogicalUri::parse("project://visual/SC02.png").unwrap(),
                    artifact_type: "image".to_owned(),
                    metadata: serde_json::Value::Null,
                },
            ),
            Err(Error::PathEscape(_))
        ));
        assert_eq!(
            state.get_job(&job.job_id).unwrap().status,
            StepStatus::Ready
        );
    }

    #[test]
    fn corrupted_cached_file_is_never_returned_as_a_cache_hit() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        let workspace = Workspace::create(&root).unwrap();
        let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = state.create_project("Corruption Test").unwrap();
        let input_hash = deterministic_input_hash(&[b"model", b"input"]);
        let job = state
            .create_job(&project.id, "tts", "S01", &input_hash)
            .unwrap();

        let source = temp.path().join("generated.wav");
        fs::write(&source, b"valid output").unwrap();

        let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
        let artifact = artifacts
            .promote_job_output(
                &mut state,
                &job.job_id,
                &source,
                LogicalUri::parse("project://audio/S01.wav").unwrap(),
                "audio",
                serde_json::Value::Null,
            )
            .unwrap();

        fs::write(
            artifacts.resolve_artifact_path(&artifact).unwrap(),
            b"tampered",
        )
        .unwrap();

        assert!(matches!(
            artifacts.lookup_verified_cache(&state, &input_hash),
            Err(Error::ArtifactHashMismatch(_))
        ));
    }
}
