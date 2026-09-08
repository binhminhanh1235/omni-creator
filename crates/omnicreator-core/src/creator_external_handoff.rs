use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    deterministic_input_hash, inspect_manual_visual_media_v1, load_latest_creator_content_scene_v1,
    load_latest_creator_content_v1, provide_manual_creator_visual_file_v1,
    provide_manual_creator_voice_bundle_v1, ArtifactStore, Error, GeneratedImageRequestV1,
    GeneratedImageResolutionV1, GeneratedImageStyleV1, ManualCreatorVisualOutcomeV1,
    ManualCreatorVoiceOutcomeV1, ManualCreatorVoiceRequestV1, ManualResultProvenanceV1,
    ManualVisualMediaKindV1, Result, StateStore, VoiceDirectionV1, VoiceTimingV1,
    CREATOR_VOICE_OPERATION_V1, GENERATED_IMAGE_OPERATION_V1,
};

pub const EXTERNAL_HANDOFF_SCHEMA_V1: &str = "omnicreator.external-handoff";
pub const EXTERNAL_HANDOFF_VERSION_V1: u32 = 1;

const EXTERNAL_VISUAL_REQUEST_DOMAIN_V1: &[u8] = b"external-generated-visual-request-v1";
const EXTERNAL_VOICE_REQUEST_DOMAIN_V1: &[u8] = b"external-voice-request-v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExternalVisualResultContractV1 {
    pub media_kind: String,
    pub accepted_mime_types: Vec<String>,
}

impl ExternalVisualResultContractV1 {
    fn image_v1() -> Self {
        Self {
            media_kind: "image".to_owned(),
            accepted_mime_types: vec![
                "image/png".to_owned(),
                "image/jpeg".to_owned(),
                "image/webp".to_owned(),
            ],
        }
    }

    fn validate_v1(&self) -> Result<()> {
        if self.media_kind != "image"
            || self.accepted_mime_types
                != ["image/png", "image/jpeg", "image/webp"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
        {
            return Err(Error::InvalidContract(
                "external generated visual result contract must require a supported still image"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExternalGeneratedVisualRequestV1 {
    pub schema: String,
    pub version: u32,
    pub project_id: String,
    pub scene_id: String,
    pub operation: String,
    pub instructions: String,
    pub generated_request: GeneratedImageRequestV1,
    pub result_contract: ExternalVisualResultContractV1,
    pub request_sha256: String,
}

impl ExternalGeneratedVisualRequestV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_common_v1(
            &self.schema,
            self.version,
            &self.project_id,
            "external visual project_id",
        )?;
        validate_identifier_v1("external visual scene_id", &self.scene_id)?;
        if self.operation != GENERATED_IMAGE_OPERATION_V1 {
            return Err(Error::InvalidContract(
                "external generated visual operation must remain visual.generate".to_owned(),
            ));
        }
        if self.instructions.trim().is_empty() {
            return Err(Error::InvalidContract(
                "external generated visual instructions must not be empty".to_owned(),
            ));
        }
        self.generated_request.validate_v1()?;
        if self.generated_request.scene.id != self.scene_id {
            return Err(Error::InvalidContract(
                "external generated visual SceneIntent does not match scene_id".to_owned(),
            ));
        }
        self.result_contract.validate_v1()?;
        validate_portable_external_value_v1(
            &serde_json::to_value(self)?,
            "external generated visual request",
        )?;
        let expected = external_visual_request_hash_v1(
            &self.project_id,
            &self.scene_id,
            &self.operation,
            &self.instructions,
            &self.generated_request,
            &self.result_contract,
        )?;
        if self.request_sha256 != expected {
            return Err(Error::InvalidContract(
                "external generated visual request_sha256 is stale".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn to_pretty_json_v1(&self) -> Result<String> {
        self.validate_v1()?;
        Ok(serde_json::to_string_pretty(self)?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExternalVoiceResultContractV1 {
    pub accepted_audio_mime_types: Vec<String>,
    pub timing_required: bool,
    pub accepted_timing_formats: Vec<String>,
}

impl ExternalVoiceResultContractV1 {
    fn canonical_v1() -> Self {
        Self {
            accepted_audio_mime_types: vec!["audio/wav".to_owned(), "audio/mpeg".to_owned()],
            timing_required: true,
            accepted_timing_formats: vec!["srt".to_owned(), "voice-timing-json".to_owned()],
        }
    }

    fn validate_v1(&self) -> Result<()> {
        if self.accepted_audio_mime_types
            != ["audio/wav", "audio/mpeg"]
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
            || !self.timing_required
            || self.accepted_timing_formats
                != ["srt", "voice-timing-json"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
        {
            return Err(Error::InvalidContract(
                "external voice result contract must require supported audio plus timing"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExternalVoiceRequestV1 {
    pub schema: String,
    pub version: u32,
    pub project_id: String,
    pub segment_id: String,
    pub operation: String,
    pub instructions: String,
    pub narration: String,
    pub voice_direction: VoiceDirectionV1,
    pub result_contract: ExternalVoiceResultContractV1,
    pub request_sha256: String,
}

impl ExternalVoiceRequestV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_common_v1(
            &self.schema,
            self.version,
            &self.project_id,
            "external voice project_id",
        )?;
        validate_identifier_v1("external voice segment_id", &self.segment_id)?;
        if self.operation != CREATOR_VOICE_OPERATION_V1 {
            return Err(Error::InvalidContract(
                "external voice operation must remain tts.generate".to_owned(),
            ));
        }
        if self.instructions.trim().is_empty() || self.narration.trim().is_empty() {
            return Err(Error::InvalidContract(
                "external voice instructions and narration must not be empty".to_owned(),
            ));
        }
        self.result_contract.validate_v1()?;
        validate_portable_external_value_v1(
            &serde_json::to_value(self)?,
            "external voice request",
        )?;
        let expected = external_voice_request_hash_v1(
            &self.project_id,
            &self.segment_id,
            &self.operation,
            &self.instructions,
            &self.narration,
            &self.voice_direction,
            &self.result_contract,
        )?;
        if self.request_sha256 != expected {
            return Err(Error::InvalidContract(
                "external voice request_sha256 is stale".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn to_pretty_json_v1(&self) -> Result<String> {
        self.validate_v1()?;
        Ok(serde_json::to_string_pretty(self)?)
    }
}

pub fn prepare_external_generated_visual_request_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    scene_id: &str,
) -> Result<ExternalGeneratedVisualRequestV1> {
    validate_identifier_v1("external visual project_id", project_id)?;
    validate_identifier_v1("external visual scene_id", scene_id)?;
    let creator = load_latest_creator_content_scene_v1(state_store, artifact_store, project_id)?
        .ok_or_else(|| {
            Error::InvalidContract(
                "external generated visual handoff requires verified Content + ScenePlan"
                    .to_owned(),
            )
        })?;
    let scene = creator
        .scene_plan
        .scenes
        .iter()
        .find(|scene| scene.id == scene_id)
        .ok_or_else(|| Error::InvalidContract(format!("creator scene not found: {scene_id}")))?;
    scene.validate_v1()?;

    let generated_request = GeneratedImageRequestV1::from_scene_v1(
        scene.clone(),
        GeneratedImageStyleV1 {
            preset: "omnicreator-external".to_owned(),
            description: None,
        },
        external_resolution_v1(&scene.aspect_ratio),
        None,
        BTreeMap::new(),
    )?;
    let instructions = "Generate one still image matching the prepared prompt and SceneIntent. Return only an image file that satisfies the result contract; OmniCreator will validate and ingest it canonically.".to_owned();
    let result_contract = ExternalVisualResultContractV1::image_v1();
    let request_sha256 = external_visual_request_hash_v1(
        project_id,
        scene_id,
        GENERATED_IMAGE_OPERATION_V1,
        &instructions,
        &generated_request,
        &result_contract,
    )?;
    let request = ExternalGeneratedVisualRequestV1 {
        schema: EXTERNAL_HANDOFF_SCHEMA_V1.to_owned(),
        version: EXTERNAL_HANDOFF_VERSION_V1,
        project_id: project_id.to_owned(),
        scene_id: scene_id.to_owned(),
        operation: GENERATED_IMAGE_OPERATION_V1.to_owned(),
        instructions,
        generated_request,
        result_contract,
        request_sha256,
    };
    request.validate_v1()?;
    Ok(request)
}

pub fn prepare_external_voice_request_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    segment_id: &str,
) -> Result<ExternalVoiceRequestV1> {
    validate_identifier_v1("external voice project_id", project_id)?;
    validate_identifier_v1("external voice segment_id", segment_id)?;
    let (content, _) = load_latest_creator_content_v1(state_store, artifact_store, project_id)?
        .ok_or_else(|| {
            Error::InvalidContract(
                "external voice handoff requires verified creator Content".to_owned(),
            )
        })?;
    let segment = content
        .segments
        .iter()
        .find(|segment| segment.id == segment_id)
        .ok_or_else(|| {
            Error::InvalidContract(format!("creator content segment not found: {segment_id}"))
        })?;
    segment.validate_v1()?;

    let instructions = "Render narration audio for this segment. Return WAV or MP3 plus SRT or OmniCreator VoiceTiming JSON whose segment_id matches this request.".to_owned();
    let result_contract = ExternalVoiceResultContractV1::canonical_v1();
    let request_sha256 = external_voice_request_hash_v1(
        project_id,
        segment_id,
        CREATOR_VOICE_OPERATION_V1,
        &instructions,
        &segment.text,
        &segment.voice_direction,
        &result_contract,
    )?;
    let request = ExternalVoiceRequestV1 {
        schema: EXTERNAL_HANDOFF_SCHEMA_V1.to_owned(),
        version: EXTERNAL_HANDOFF_VERSION_V1,
        project_id: project_id.to_owned(),
        segment_id: segment_id.to_owned(),
        operation: CREATOR_VOICE_OPERATION_V1.to_owned(),
        instructions,
        narration: segment.text.clone(),
        voice_direction: segment.voice_direction.clone(),
        result_contract,
        request_sha256,
    };
    request.validate_v1()?;
    Ok(request)
}

pub fn provide_external_generated_visual_result_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    request: &ExternalGeneratedVisualRequestV1,
    source: impl AsRef<Path>,
    source_label: impl Into<String>,
    replace_existing: bool,
) -> Result<ManualCreatorVisualOutcomeV1> {
    request.validate_v1()?;
    let current = prepare_external_generated_visual_request_v1(
        state_store,
        artifact_store,
        &request.project_id,
        &request.scene_id,
    )?;
    if current.request_sha256 != request.request_sha256 {
        return Err(Error::InvalidContract(
            "external generated visual request is stale against current canonical ScenePlan"
                .to_owned(),
        ));
    }
    let media = inspect_manual_visual_media_v1(source.as_ref())?;
    if media.media_kind != ManualVisualMediaKindV1::Image
        || !request
            .result_contract
            .accepted_mime_types
            .iter()
            .any(|mime| mime == &media.mime_type)
    {
        return Err(Error::InvalidContract(format!(
            "external generated visual result {} does not satisfy the prepared still-image contract",
            media.mime_type
        )));
    }
    provide_manual_creator_visual_file_v1(
        state_store,
        artifact_store,
        &request.project_id,
        &request.scene_id,
        source,
        ManualResultProvenanceV1::external(source_label, Some(request.request_sha256.clone())),
        replace_existing,
    )
}

pub fn provide_external_voice_result_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    request: &ExternalVoiceRequestV1,
    audio_path: impl AsRef<Path>,
    timing: VoiceTimingV1,
    source_label: impl Into<String>,
    replace_existing: bool,
) -> Result<ManualCreatorVoiceOutcomeV1> {
    request.validate_v1()?;
    let current = prepare_external_voice_request_v1(
        state_store,
        artifact_store,
        &request.project_id,
        &request.segment_id,
    )?;
    if current.request_sha256 != request.request_sha256 {
        return Err(Error::InvalidContract(
            "external voice request is stale against current canonical Content".to_owned(),
        ));
    }
    provide_manual_creator_voice_bundle_v1(
        state_store,
        artifact_store,
        ManualCreatorVoiceRequestV1 {
            project_id: &request.project_id,
            segment_id: &request.segment_id,
            audio_path: audio_path.as_ref(),
            timing,
            provenance: ManualResultProvenanceV1::external(
                source_label,
                Some(request.request_sha256.clone()),
            ),
            replace_existing,
        },
    )
}

fn external_resolution_v1(aspect_ratio: &str) -> GeneratedImageResolutionV1 {
    match aspect_ratio.trim() {
        "9:16" | "9/16" => GeneratedImageResolutionV1 {
            width: 720,
            height: 1280,
        },
        "1:1" | "1/1" => GeneratedImageResolutionV1 {
            width: 1024,
            height: 1024,
        },
        "4:5" | "4/5" => GeneratedImageResolutionV1 {
            width: 1024,
            height: 1280,
        },
        _ => GeneratedImageResolutionV1 {
            width: 1280,
            height: 720,
        },
    }
}

fn external_visual_request_hash_v1(
    project_id: &str,
    scene_id: &str,
    operation: &str,
    instructions: &str,
    generated_request: &GeneratedImageRequestV1,
    result_contract: &ExternalVisualResultContractV1,
) -> Result<String> {
    let generated = serde_json::to_vec(generated_request)?;
    let result = serde_json::to_vec(result_contract)?;
    Ok(deterministic_input_hash(&[
        EXTERNAL_VISUAL_REQUEST_DOMAIN_V1,
        project_id.as_bytes(),
        scene_id.as_bytes(),
        operation.as_bytes(),
        instructions.as_bytes(),
        generated.as_slice(),
        result.as_slice(),
    ]))
}

fn external_voice_request_hash_v1(
    project_id: &str,
    segment_id: &str,
    operation: &str,
    instructions: &str,
    narration: &str,
    voice_direction: &VoiceDirectionV1,
    result_contract: &ExternalVoiceResultContractV1,
) -> Result<String> {
    let direction = serde_json::to_vec(voice_direction)?;
    let result = serde_json::to_vec(result_contract)?;
    Ok(deterministic_input_hash(&[
        EXTERNAL_VOICE_REQUEST_DOMAIN_V1,
        project_id.as_bytes(),
        segment_id.as_bytes(),
        operation.as_bytes(),
        instructions.as_bytes(),
        narration.as_bytes(),
        direction.as_slice(),
        result.as_slice(),
    ]))
}

fn validate_common_v1(schema: &str, version: u32, project_id: &str, label: &str) -> Result<()> {
    if schema != EXTERNAL_HANDOFF_SCHEMA_V1 || version != EXTERNAL_HANDOFF_VERSION_V1 {
        return Err(Error::InvalidContract(
            "unsupported external-handoff schema/version".to_owned(),
        ));
    }
    validate_identifier_v1(label, project_id)
}

fn validate_identifier_v1(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 256 || value.contains('\0') {
        return Err(Error::InvalidContract(format!(
            "{label} must be a non-empty portable identifier"
        )));
    }
    Ok(())
}

fn validate_portable_external_value_v1(value: &serde_json::Value, label: &str) -> Result<()> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let normalized = key.to_ascii_lowercase().replace('-', "_");
                let forbidden = [
                    "secret",
                    "password",
                    "token",
                    "api_key",
                    "authorization",
                    "cookie",
                    "credential",
                    "provider_id",
                    "plugin_id",
                    "session_id",
                    "worker_id",
                    "device_id",
                    "runtime_path",
                    "workspace_path",
                ];
                if forbidden.iter().any(|needle| normalized.contains(needle)) {
                    return Err(Error::InvalidContract(format!(
                        "{label} contains provider-private or secret-like field {key}"
                    )));
                }
                validate_portable_external_value_v1(child, label)?;
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                validate_portable_external_value_v1(child, label)?;
            }
        }
        serde_json::Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.starts_with('/')
                || trimmed.starts_with("\\\\")
                || trimmed.starts_with("file://")
                || looks_like_windows_absolute_v1(trimmed)
            {
                return Err(Error::InvalidContract(format!(
                    "{label} must not contain absolute machine paths"
                )));
            }
        }
        _ => {}
    }
    Ok(())
}

fn looks_like_windows_absolute_v1(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}
