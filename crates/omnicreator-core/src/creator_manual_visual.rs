use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{
    deterministic_input_hash, ingest_manual_result_file_v1, load_latest_creator_content_scene_v1,
    Artifact, ArtifactStore, Error, LogicalUri, ManualResultIngestRequestV1,
    ManualResultOutcomeV1, ManualResultProvenanceV1, Result, StateStore, StepStatus,
    WorkflowStep, CREATOR_STEP_VISUAL_PREPARE_V1, CREATOR_WORKFLOW_UNIT_PROJECT_V1,
    MANUAL_RESULT_SCHEMA_V1, MANUAL_RESULT_VERSION_V1,
};

pub const MANUAL_VISUAL_SCHEMA_V1: &str = "omnicreator.manual-visual";
pub const MANUAL_VISUAL_VERSION_V1: u32 = 1;
const MANUAL_VISUAL_PREFIX_BYTES_V1: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualVisualMediaKindV1 {
    Image,
    Video,
}

impl ManualVisualMediaKindV1 {
    fn artifact_type_v1(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ManualVisualMediaMetadataV1 {
    pub schema: String,
    pub version: u32,
    pub media_kind: ManualVisualMediaKindV1,
    pub extension: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_seconds: Option<f64>,
}

impl ManualVisualMediaMetadataV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != MANUAL_VISUAL_SCHEMA_V1 || self.version != MANUAL_VISUAL_VERSION_V1 {
            return Err(Error::InvalidContract(
                "unsupported manual visual metadata schema/version".to_owned(),
            ));
        }
        if self.extension.trim().is_empty()
            || self.mime_type.trim().is_empty()
            || self.size_bytes == 0
        {
            return Err(Error::InvalidContract(
                "manual visual extension, mime_type and non-zero size are required".to_owned(),
            ));
        }
        if self.width.is_some_and(|value| value == 0)
            || self.height.is_some_and(|value| value == 0)
            || self.duration_seconds.is_some_and(|value| value <= 0.0)
        {
            return Err(Error::InvalidContract(
                "manual visual dimensions/duration must be positive when present".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ManualCreatorVisualOutcomeV1 {
    pub scene_id: String,
    pub artifact: Artifact,
    pub media: ManualVisualMediaMetadataV1,
    pub ingestion: ManualResultOutcomeV1,
    pub visual_stage_complete: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CreatorSceneVisualStateV1 {
    pub scene_id: String,
    pub selected_artifact: Option<Artifact>,
    pub verified: bool,
}

pub fn inspect_manual_visual_media_v1(
    path: impl AsRef<Path>,
) -> Result<ManualVisualMediaMetadataV1> {
    let path = path.as_ref();
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(Error::InvalidContract(
            "manual visual import must be a non-empty regular file".to_owned(),
        ));
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| Error::InvalidContract("manual visual file has no extension".to_owned()))?;

    let mut file = fs::File::open(path)?;
    let mut prefix = Vec::with_capacity(
        usize::try_from(metadata.len().min(MANUAL_VISUAL_PREFIX_BYTES_V1 as u64))
            .unwrap_or(MANUAL_VISUAL_PREFIX_BYTES_V1),
    );
    file.by_ref()
        .take(MANUAL_VISUAL_PREFIX_BYTES_V1 as u64)
        .read_to_end(&mut prefix)?;
    file.seek(SeekFrom::Start(0))?;

    let (media_kind, mime_type, width, height) = match extension.as_str() {
        "png" => {
            let (width, height) = png_dimensions_v1(&prefix)?;
            (
                ManualVisualMediaKindV1::Image,
                "image/png".to_owned(),
                Some(width),
                Some(height),
            )
        }
        "jpg" | "jpeg" => {
            validate_jpeg_v1(&prefix)?;
            let dimensions = jpeg_dimensions_v1(&prefix);
            (
                ManualVisualMediaKindV1::Image,
                "image/jpeg".to_owned(),
                dimensions.map(|value| value.0),
                dimensions.map(|value| value.1),
            )
        }
        "gif" => {
            let (width, height) = gif_dimensions_v1(&prefix)?;
            (
                ManualVisualMediaKindV1::Image,
                "image/gif".to_owned(),
                Some(width),
                Some(height),
            )
        }
        "webp" => {
            validate_webp_v1(&prefix)?;
            let dimensions = webp_dimensions_v1(&prefix);
            (
                ManualVisualMediaKindV1::Image,
                "image/webp".to_owned(),
                dimensions.map(|value| value.0),
                dimensions.map(|value| value.1),
            )
        }
        "mp4" | "m4v" => {
            validate_iso_bmff_v1(&prefix)?;
            (
                ManualVisualMediaKindV1::Video,
                "video/mp4".to_owned(),
                None,
                None,
            )
        }
        "mov" => {
            validate_iso_bmff_v1(&prefix)?;
            (
                ManualVisualMediaKindV1::Video,
                "video/quicktime".to_owned(),
                None,
                None,
            )
        }
        "webm" | "mkv" => {
            validate_ebml_v1(&prefix)?;
            let mime = if extension == "webm" {
                "video/webm"
            } else {
                "video/x-matroska"
            };
            (
                ManualVisualMediaKindV1::Video,
                mime.to_owned(),
                None,
                None,
            )
        }
        "avi" => {
            validate_avi_v1(&prefix)?;
            (
                ManualVisualMediaKindV1::Video,
                "video/x-msvideo".to_owned(),
                None,
                None,
            )
        }
        other => {
            return Err(Error::InvalidContract(format!(
                "manual visual extension .{other} is not supported"
            )))
        }
    };

    let result = ManualVisualMediaMetadataV1 {
        schema: MANUAL_VISUAL_SCHEMA_V1.to_owned(),
        version: MANUAL_VISUAL_VERSION_V1,
        media_kind,
        extension,
        mime_type,
        size_bytes: metadata.len(),
        width,
        height,
        duration_seconds: None,
    };
    result.validate_v1()?;
    Ok(result)
}

pub fn provide_manual_creator_visual_file_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    scene_id: &str,
    path: impl AsRef<Path>,
    provenance: ManualResultProvenanceV1,
    replace_existing: bool,
) -> Result<ManualCreatorVisualOutcomeV1> {
    persist_manual_creator_visual_v1(
        state_store,
        artifact_store,
        project_id,
        scene_id,
        path.as_ref(),
        provenance,
        replace_existing,
        None,
    )
}

pub fn choose_creator_visual_from_asset_library_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    scene_id: &str,
    source_artifact_id: &str,
    replace_existing: bool,
) -> Result<ManualCreatorVisualOutcomeV1> {
    let source = state_store.get_artifact(source_artifact_id)?;
    if !matches!(source.artifact_type.to_ascii_lowercase().as_str(), "image" | "video") {
        return Err(Error::InvalidArtifact(
            "Asset Library visual selection must be an image or video artifact".to_owned(),
        ));
    }
    if !artifact_store.verify_artifact(&source)? {
        return Err(Error::InvalidArtifact(format!(
            "Asset Library artifact {source_artifact_id} is not physically verifiable"
        )));
    }
    let path = artifact_store.resolve_artifact_path(&source)?;
    persist_manual_creator_visual_v1(
        state_store,
        artifact_store,
        project_id,
        scene_id,
        &path,
        ManualResultProvenanceV1::manual_editor(),
        replace_existing,
        Some(&source),
    )
}

pub fn creator_scene_visual_states_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
) -> Result<Vec<CreatorSceneVisualStateV1>> {
    let creator = load_latest_creator_content_scene_v1(state_store, artifact_store, project_id)?
        .ok_or_else(|| {
            Error::InvalidContract(
                "creator visual status requires verified Content + ScenePlan".to_owned(),
            )
        })?;
    creator
        .scene_plan
        .scenes
        .iter()
        .map(|scene| {
            let selected =
                latest_verified_visual_for_scene_v1(state_store, artifact_store, project_id, &scene.id)?;
            Ok(CreatorSceneVisualStateV1 {
                scene_id: scene.id.clone(),
                verified: selected.is_some(),
                selected_artifact: selected,
            })
        })
        .collect()
}

pub fn reconcile_creator_visual_aggregate_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
) -> Result<bool> {
    let states = creator_scene_visual_states_v1(state_store, artifact_store, project_id)?;
    let complete = !states.is_empty() && states.iter().all(|state| state.verified);
    let step = visual_aggregate_step_v1(state_store, project_id)?;
    let current = state_store.get_step(&step.step_id)?;

    if complete {
        match current.status {
            StepStatus::Succeeded => {}
            StepStatus::Ready => {
                state_store.set_step_status(&current.step_id, StepStatus::Succeeded)?;
            }
            StepStatus::Stale
            | StepStatus::Retryable
            | StepStatus::Failed
            | StepStatus::Cancelled
            | StepStatus::Skipped => {
                state_store.set_step_status(&current.step_id, StepStatus::Ready)?;
                state_store.set_step_status(&current.step_id, StepStatus::Succeeded)?;
            }
            StepStatus::NotReady => {
                return Err(Error::InvalidTransition(
                    "manual visual aggregate is still waiting on ScenePlan".to_owned(),
                ))
            }
            StepStatus::Running | StepStatus::Queued => {
                return Err(Error::InvalidTransition(
                    "manual visual aggregate cannot complete while automatic execution is active"
                        .to_owned(),
                ))
            }
            StepStatus::Fatal => {
                return Err(Error::InvalidTransition(
                    "manual visual aggregate is FATAL".to_owned(),
                ))
            }
        }
        state_store.refresh_ready_steps(project_id)?;
    }
    Ok(complete)
}

fn persist_manual_creator_visual_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    scene_id: &str,
    source_path: &Path,
    provenance: ManualResultProvenanceV1,
    replace_existing: bool,
    source_library_artifact: Option<&Artifact>,
) -> Result<ManualCreatorVisualOutcomeV1> {
    state_store.get_project(project_id)?;
    provenance.validate_v1()?;
    let creator = load_latest_creator_content_scene_v1(state_store, artifact_store, project_id)?
        .ok_or_else(|| {
            Error::InvalidContract(
                "manual visual requires verified Content + ScenePlan".to_owned(),
            )
        })?;
    let scene = creator
        .scene_plan
        .scenes
        .iter()
        .find(|scene| scene.id == scene_id)
        .ok_or_else(|| Error::InvalidContract(format!("creator scene not found: {scene_id}")))?;
    scene.validate_v1()?;

    let media = inspect_manual_visual_media_v1(source_path)?;
    let (source_sha256, _) = crate::fs_util::sha256_file(source_path)?;
    let target_uri = manual_visual_target_uri_v1(
        scene_id,
        &media.extension,
        &source_sha256,
        &creator.scene_plan_artifact.sha256,
        &provenance,
    )?;
    let source_library_artifact_id =
        source_library_artifact.map(|artifact| artifact.artifact_id.clone());
    let source_library_sha256 =
        source_library_artifact.map(|artifact| artifact.sha256.clone());

    let request = ManualResultIngestRequestV1 {
        schema: MANUAL_RESULT_SCHEMA_V1.to_owned(),
        version: MANUAL_RESULT_VERSION_V1,
        project_id: project_id.to_owned(),
        workflow_step: CREATOR_STEP_VISUAL_PREPARE_V1.to_owned(),
        workflow_unit: CREATOR_WORKFLOW_UNIT_PROJECT_V1.to_owned(),
        job_step: CREATOR_STEP_VISUAL_PREPARE_V1.to_owned(),
        job_unit: scene_id.to_owned(),
        artifact_type: media.media_kind.artifact_type_v1().to_owned(),
        target_uri,
        provenance,
        stage_metadata: serde_json::json!({
            "contract": MANUAL_VISUAL_SCHEMA_V1,
            "schema_version": MANUAL_VISUAL_VERSION_V1,
            "mode": "manual",
            "scene_id": scene_id,
            "scene_plan_sha256": &creator.scene_plan_artifact.sha256,
            "media_kind": media.media_kind,
            "extension": &media.extension,
            "mime_type": &media.mime_type,
            "size_bytes": media.size_bytes,
            "width": media.width,
            "height": media.height,
            "duration": media.duration_seconds,
            "source_library_artifact_id": source_library_artifact_id,
            "source_library_sha256": source_library_sha256,
        }),
        replace_existing,
        complete_workflow_step: false,
    };
    let ingestion =
        ingest_manual_result_file_v1(state_store, artifact_store, &request, source_path)?;
    let artifact = ingestion.artifact.clone();
    if artifact.project_id.as_deref() != Some(project_id)
        || artifact.artifact_type != media.media_kind.artifact_type_v1()
        || !artifact_store.verify_artifact(&artifact)?
    {
        return Err(Error::InvalidArtifact(
            "manual visual did not promote a verified canonical scene artifact".to_owned(),
        ));
    }
    let visual_stage_complete =
        reconcile_creator_visual_aggregate_v1(state_store, artifact_store, project_id)?;

    Ok(ManualCreatorVisualOutcomeV1 {
        scene_id: scene_id.to_owned(),
        artifact,
        media,
        ingestion,
        visual_stage_complete,
    })
}

fn latest_verified_visual_for_scene_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    scene_id: &str,
) -> Result<Option<Artifact>> {
    let mut candidates = Vec::new();
    for job in state_store.list_project_jobs(project_id)? {
        if job.step != CREATOR_STEP_VISUAL_PREPARE_V1
            || job.unit != scene_id
            || job.status != StepStatus::Succeeded
        {
            continue;
        }
        let Some(artifact_id) = job.selected_artifact.as_deref() else {
            continue;
        };
        let artifact = state_store.get_artifact(artifact_id)?;
        if artifact.project_id.as_deref() != Some(project_id)
            || artifact.producer_job.as_deref() != Some(job.job_id.as_str())
            || artifact.input_hash.as_deref() != Some(job.input_hash.as_str())
            || !matches!(artifact.artifact_type.to_ascii_lowercase().as_str(), "image" | "video")
            || !artifact_store.verify_artifact(&artifact)?
        {
            continue;
        }
        candidates.push((artifact.created_at, artifact.artifact_id.clone(), artifact));
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    Ok(candidates.pop().map(|(_, _, artifact)| artifact))
}

fn visual_aggregate_step_v1(
    state_store: &StateStore,
    project_id: &str,
) -> Result<WorkflowStep> {
    state_store
        .list_project_steps(project_id)?
        .into_iter()
        .find(|step| {
            step.step == CREATOR_STEP_VISUAL_PREPARE_V1
                && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1
        })
        .ok_or_else(|| {
            Error::InvalidContract(
                "creator visual aggregate workflow step is missing".to_owned(),
            )
        })
}

fn manual_visual_target_uri_v1(
    scene_id: &str,
    extension: &str,
    source_sha256: &str,
    scene_plan_sha256: &str,
    provenance: &ManualResultProvenanceV1,
) -> Result<LogicalUri> {
    if scene_id.trim().is_empty()
        || extension.trim().is_empty()
        || scene_id.contains('/')
        || scene_id.contains('\\')
        || extension.contains('/')
        || extension.contains('\\')
    {
        return Err(Error::InvalidContract(
            "manual visual target scene_id/extension must be portable identifiers".to_owned(),
        ));
    }
    let provenance_json = serde_json::to_vec(provenance)?;
    let id = deterministic_input_hash(&[
        b"manual-visual-target-v1",
        scene_id.as_bytes(),
        extension.as_bytes(),
        source_sha256.as_bytes(),
        scene_plan_sha256.as_bytes(),
        provenance_json.as_slice(),
    ]);
    LogicalUri::parse(&format!(
        "project://visual/manual/{scene_id}/{id}.{extension}"
    ))
}

fn png_dimensions_v1(bytes: &[u8]) -> Result<(u32, u32)> {
    const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return Err(Error::InvalidContract(
            "manual PNG does not contain a valid PNG/IHDR header".to_owned(),
        ));
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width == 0 || height == 0 {
        return Err(Error::InvalidContract(
            "manual PNG dimensions must be positive".to_owned(),
        ));
    }
    Ok((width, height))
}

fn validate_jpeg_v1(bytes: &[u8]) -> Result<()> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return Err(Error::InvalidContract(
            "manual JPEG does not contain a valid SOI header".to_owned(),
        ));
    }
    Ok(())
}

fn jpeg_dimensions_v1(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut index = 2usize;
    while index + 4 <= bytes.len() {
        if bytes[index] != 0xff {
            index += 1;
            continue;
        }
        while index < bytes.len() && bytes[index] == 0xff {
            index += 1;
        }
        if index >= bytes.len() {
            return None;
        }
        let marker = bytes[index];
        index += 1;
        if marker == 0xd8 || marker == 0xd9 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if index + 2 > bytes.len() {
            return None;
        }
        let length = usize::from(u16::from_be_bytes([bytes[index], bytes[index + 1]]));
        if length < 2 || index + length > bytes.len() {
            return None;
        }
        if matches!(
            marker,
            0xc0 | 0xc1 | 0xc2 | 0xc3 | 0xc5 | 0xc6 | 0xc7 | 0xc9 | 0xca | 0xcb | 0xcd | 0xce | 0xcf
        ) && length >= 7
        {
            let height = u32::from(u16::from_be_bytes([bytes[index + 3], bytes[index + 4]]));
            let width = u32::from(u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]));
            if width > 0 && height > 0 {
                return Some((width, height));
            }
        }
        index += length;
    }
    None
}

fn gif_dimensions_v1(bytes: &[u8]) -> Result<(u32, u32)> {
    if bytes.len() < 10 || !matches!(&bytes[..6], b"GIF87a" | b"GIF89a") {
        return Err(Error::InvalidContract(
            "manual GIF does not contain a valid GIF header".to_owned(),
        ));
    }
    let width = u32::from(u16::from_le_bytes([bytes[6], bytes[7]]));
    let height = u32::from(u16::from_le_bytes([bytes[8], bytes[9]]));
    if width == 0 || height == 0 {
        return Err(Error::InvalidContract(
            "manual GIF dimensions must be positive".to_owned(),
        ));
    }
    Ok((width, height))
}

fn validate_webp_v1(bytes: &[u8]) -> Result<()> {
    if bytes.len() < 16 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err(Error::InvalidContract(
            "manual WebP does not contain a valid RIFF/WEBP header".to_owned(),
        ));
    }
    Ok(())
}

fn webp_dimensions_v1(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() >= 30 && &bytes[12..16] == b"VP8X" {
        let width = 1
            + u32::from(bytes[24])
            + (u32::from(bytes[25]) << 8)
            + (u32::from(bytes[26]) << 16);
        let height = 1
            + u32::from(bytes[27])
            + (u32::from(bytes[28]) << 8)
            + (u32::from(bytes[29]) << 16);
        return Some((width, height));
    }
    if bytes.len() >= 25 && &bytes[12..16] == b"VP8L" && bytes[20] == 0x2f {
        let b1 = u32::from(bytes[21]);
        let b2 = u32::from(bytes[22]);
        let b3 = u32::from(bytes[23]);
        let b4 = u32::from(bytes[24]);
        let width = 1 + (((b2 & 0x3f) << 8) | b1);
        let height = 1 + (((b4 & 0x0f) << 10) | (b3 << 2) | ((b2 & 0xc0) >> 6));
        return Some((width, height));
    }
    if bytes.len() >= 30 && &bytes[12..16] == b"VP8 " && &bytes[23..26] == b"\x9d\x01\x2a" {
        let width = u32::from(u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3fff);
        let height = u32::from(u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3fff);
        if width > 0 && height > 0 {
            return Some((width, height));
        }
    }
    None
}

fn validate_iso_bmff_v1(bytes: &[u8]) -> Result<()> {
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return Err(Error::InvalidContract(
            "manual MP4/MOV does not contain an ISO-BMFF ftyp header".to_owned(),
        ));
    }
    Ok(())
}

fn validate_ebml_v1(bytes: &[u8]) -> Result<()> {
    if bytes.len() < 4 || bytes[..4] != [0x1a, 0x45, 0xdf, 0xa3] {
        return Err(Error::InvalidContract(
            "manual WebM/MKV does not contain an EBML header".to_owned(),
        ));
    }
    Ok(())
}

fn validate_avi_v1(bytes: &[u8]) -> Result<()> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"AVI " {
        return Err(Error::InvalidContract(
            "manual AVI does not contain a RIFF/AVI header".to_owned(),
        ));
    }
    Ok(())
}
