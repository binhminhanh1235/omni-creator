use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Error, LogicalUri, Result, StepStatus};

pub const PROJECT_IR_SCHEMA: &str = "omnicreator.project-ir";
pub const PROJECT_IR_SCHEMA_VERSION: u32 = 1;
pub const SEGMENT_SCHEMA: &str = "omnicreator.segment";
pub const SEGMENT_SCHEMA_VERSION: u32 = 1;
pub const SCENE_INTENT_SCHEMA: &str = "omnicreator.scene-intent";
pub const SCENE_INTENT_SCHEMA_VERSION: u32 = 1;
pub const ASSET_SCHEMA: &str = "omnicreator.asset";
pub const ASSET_SCHEMA_VERSION: u32 = 1;
pub const ARTIFACT_IR_SCHEMA: &str = "omnicreator.artifact";
pub const ARTIFACT_IR_SCHEMA_VERSION: u32 = 1;
pub const STEP_IR_SCHEMA: &str = "omnicreator.step";
pub const STEP_IR_SCHEMA_VERSION: u32 = 1;
pub const JOB_IR_SCHEMA: &str = "omnicreator.job";
pub const JOB_IR_SCHEMA_VERSION: u32 = 1;
pub const ATTEMPT_IR_SCHEMA: &str = "omnicreator.attempt";
pub const ATTEMPT_IR_SCHEMA_VERSION: u32 = 1;
pub const COMPUTE_CAPABILITIES_SCHEMA: &str = "omnicreator.compute-capabilities";
pub const COMPUTE_CAPABILITIES_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectIrV1 {
    pub schema: String,
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub studio_pack: Option<String>,
    pub channel_profile: Option<String>,
    pub script_version: i64,
    pub production_lock: bool,
    #[serde(default)]
    pub segments: Vec<SegmentV1>,
    #[serde(default)]
    pub scene_intents: Vec<SceneIntentV1>,
    #[serde(default)]
    pub assets: Vec<AssetV1>,
}

impl ProjectIrV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_schema(
            &self.schema,
            self.schema_version,
            PROJECT_IR_SCHEMA,
            PROJECT_IR_SCHEMA_VERSION,
        )?;
        require_non_empty("project id", &self.id)?;
        require_non_empty("project title", &self.title)?;
        if self.script_version < 1 {
            return Err(Error::InvalidContract(
                "project script_version must be >= 1".to_owned(),
            ));
        }
        for segment in &self.segments {
            segment.validate_v1()?;
        }
        for scene in &self.scene_intents {
            scene.validate_v1()?;
        }
        for asset in &self.assets {
            asset.validate_v1()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SegmentV1 {
    pub schema: String,
    pub schema_version: u32,
    pub id: String,
    pub order: u32,
    pub text: String,
    #[serde(default)]
    pub voice_direction: VoiceDirectionV1,
}

impl SegmentV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_schema(
            &self.schema,
            self.schema_version,
            SEGMENT_SCHEMA,
            SEGMENT_SCHEMA_VERSION,
        )?;
        require_non_empty("segment id", &self.id)?;
        require_non_empty("segment text", &self.text)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct VoiceDirectionV1 {
    pub tone: Option<String>,
    pub pace: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneIntentV1 {
    pub schema: String,
    pub schema_version: u32,
    pub id: String,
    pub segment_id: String,
    pub narration: String,
    pub purpose: String,
    pub scene_type: String,
    pub emotion_before: Option<String>,
    pub emotion_after: Option<String>,
    pub duration_hint: Option<f64>,
    #[serde(default)]
    pub visual_ideas: Vec<String>,
    #[serde(default)]
    pub search_queries: Vec<String>,
    #[serde(default)]
    pub avoid: Vec<String>,
    #[serde(default)]
    pub continuity: BTreeMap<String, serde_json::Value>,
    pub aspect_ratio: String,
}

impl SceneIntentV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_schema(
            &self.schema,
            self.schema_version,
            SCENE_INTENT_SCHEMA,
            SCENE_INTENT_SCHEMA_VERSION,
        )?;
        require_non_empty("scene id", &self.id)?;
        require_non_empty("scene segment_id", &self.segment_id)?;
        require_non_empty("scene narration", &self.narration)?;
        require_non_empty("scene purpose", &self.purpose)?;
        require_non_empty("scene_type", &self.scene_type)?;
        require_non_empty("aspect_ratio", &self.aspect_ratio)?;
        if self.duration_hint.is_some_and(|duration| duration <= 0.0) {
            return Err(Error::InvalidContract(
                "scene duration_hint must be positive".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssetV1 {
    pub schema: String,
    pub schema_version: u32,
    pub asset_id: String,
    pub asset_type: String,
    pub uri: LogicalUri,
    pub source_provider: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<f64>,
    pub sha256: String,
    #[serde(default)]
    pub provenance: BTreeMap<String, serde_json::Value>,
}

impl AssetV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_schema(
            &self.schema,
            self.schema_version,
            ASSET_SCHEMA,
            ASSET_SCHEMA_VERSION,
        )?;
        require_non_empty("asset_id", &self.asset_id)?;
        require_non_empty("asset_type", &self.asset_type)?;
        validate_sha256(&self.sha256)?;
        if self.duration.is_some_and(|duration| duration <= 0.0) {
            return Err(Error::InvalidContract(
                "asset duration must be positive".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtifactIrV1 {
    pub schema: String,
    pub schema_version: u32,
    pub artifact_id: String,
    pub artifact_type: String,
    pub uri: LogicalUri,
    pub sha256: String,
    pub size_bytes: u64,
    pub producer_job: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl ArtifactIrV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_schema(
            &self.schema,
            self.schema_version,
            ARTIFACT_IR_SCHEMA,
            ARTIFACT_IR_SCHEMA_VERSION,
        )?;
        require_non_empty("artifact_id", &self.artifact_id)?;
        require_non_empty("artifact_type", &self.artifact_type)?;
        validate_sha256(&self.sha256)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepIrV1 {
    pub schema: String,
    pub schema_version: u32,
    pub step_id: String,
    pub project_id: String,
    pub step: String,
    pub unit: String,
    pub status: StepStatus,
    pub input_hash: Option<String>,
}

impl StepIrV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_schema(
            &self.schema,
            self.schema_version,
            STEP_IR_SCHEMA,
            STEP_IR_SCHEMA_VERSION,
        )?;
        require_non_empty("step_id", &self.step_id)?;
        require_non_empty("step project_id", &self.project_id)?;
        require_non_empty("step key", &self.step)?;
        require_non_empty("step unit", &self.unit)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobIrV1 {
    pub schema: String,
    pub schema_version: u32,
    pub job_id: String,
    pub project_id: String,
    pub step: String,
    pub unit: String,
    pub status: StepStatus,
    pub input_hash: String,
    pub selected_attempt: Option<String>,
    pub selected_artifact: Option<String>,
}

impl JobIrV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_schema(
            &self.schema,
            self.schema_version,
            JOB_IR_SCHEMA,
            JOB_IR_SCHEMA_VERSION,
        )?;
        require_non_empty("job_id", &self.job_id)?;
        require_non_empty("job project_id", &self.project_id)?;
        require_non_empty("job step", &self.step)?;
        require_non_empty("job unit", &self.unit)?;
        require_non_empty("job input_hash", &self.input_hash)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AttemptIrV1 {
    pub schema: String,
    pub schema_version: u32,
    pub attempt_id: String,
    pub job_id: String,
    pub worker: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub runtime_seconds: Option<f64>,
    pub status: StepStatus,
    pub error_code: Option<String>,
}

impl AttemptIrV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_schema(
            &self.schema,
            self.schema_version,
            ATTEMPT_IR_SCHEMA,
            ATTEMPT_IR_SCHEMA_VERSION,
        )?;
        require_non_empty("attempt_id", &self.attempt_id)?;
        require_non_empty("attempt job_id", &self.job_id)?;
        if self
            .runtime_seconds
            .is_some_and(|runtime_seconds| runtime_seconds < 0.0)
        {
            return Err(Error::InvalidContract(
                "attempt runtime_seconds must not be negative".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeProviderCapabilitiesV1 {
    pub schema: String,
    pub schema_version: u32,
    pub provider_id: String,
    #[serde(default)]
    pub devices: Vec<ComputeDeviceV1>,
    #[serde(default)]
    pub model_groups: Vec<String>,
    pub max_parallel_jobs: Option<u32>,
}

impl ComputeProviderCapabilitiesV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_schema(
            &self.schema,
            self.schema_version,
            COMPUTE_CAPABILITIES_SCHEMA,
            COMPUTE_CAPABILITIES_SCHEMA_VERSION,
        )?;
        require_non_empty("compute provider_id", &self.provider_id)?;
        if self
            .max_parallel_jobs
            .is_some_and(|max_parallel_jobs| max_parallel_jobs == 0)
        {
            return Err(Error::InvalidContract(
                "max_parallel_jobs must be positive when present".to_owned(),
            ));
        }
        for device in &self.devices {
            device.validate_v1()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeDeviceV1 {
    pub id: String,
    pub device_type: String,
    pub model: Option<String>,
    pub memory_mb: Option<u64>,
}

impl ComputeDeviceV1 {
    fn validate_v1(&self) -> Result<()> {
        require_non_empty("compute device id", &self.id)?;
        require_non_empty("compute device_type", &self.device_type)
    }
}

pub(crate) fn validate_schema(
    schema: &str,
    schema_version: u32,
    expected_schema: &str,
    expected_version: u32,
) -> Result<()> {
    if schema != expected_schema {
        return Err(Error::InvalidContract(format!(
            "expected schema {expected_schema}, found {schema}"
        )));
    }
    if schema_version != expected_version {
        return Err(Error::InvalidContract(format!(
            "unsupported {expected_schema} schema version {schema_version}; expected {expected_version}"
        )));
    }
    Ok(())
}

fn require_non_empty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidContract(format!("{label} must not be empty")));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::InvalidContract(
            "sha256 must be exactly 64 hexadecimal characters".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_schema_version_is_rejected() {
        let scene = SceneIntentV1 {
            schema: SCENE_INTENT_SCHEMA.to_owned(),
            schema_version: 2,
            id: "SC01".to_owned(),
            segment_id: "S01".to_owned(),
            narration: "Narration".to_owned(),
            purpose: "Purpose".to_owned(),
            scene_type: "conceptual".to_owned(),
            emotion_before: None,
            emotion_after: None,
            duration_hint: Some(5.0),
            visual_ideas: vec![],
            search_queries: vec![],
            avoid: vec![],
            continuity: BTreeMap::new(),
            aspect_ratio: "16:9".to_owned(),
        };

        assert!(matches!(
            scene.validate_v1(),
            Err(Error::InvalidContract(_))
        ));
    }
}
