use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
    assemble_creator_production_pack_v1, import_manual_voice_timing_file_v1,
    inspect_manual_voice_audio_v1, load_latest_creator_content_scene_v1,
    provide_manual_creator_visual_file_v1, provide_manual_creator_voice_bundle_v1, Artifact,
    ArtifactStore, CreatorProductionPackOptionsV1, CreatorProductionPackOutcomeV1, Error,
    FcpxmlExportProfileV1, Job, ManualCreatorVisualOutcomeV1, ManualCreatorVoiceOutcomeV1,
    ManualCreatorVoiceRequestV1, ManualResultProvenanceV1, ProductionPackageExportOutcomeV1,
    ProductionPackageExporterV1, Result, StateStore, StepStatus, VoiceTakeV1,
    CREATOR_STEP_VISUAL_PREPARE_V1, CREATOR_TTS_STEP_V1, VOICE_AUDIO_ARTIFACT_TYPE_V1,
    VOICE_TIMING_ARTIFACT_TYPE_V1,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProductionRecoveryArtifactKindV1 {
    Visual,
    Audio,
    Timing,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProductionRecoveryArtifactStateV1 {
    Verified,
    Missing,
    Invalid,
    Unselected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProductionRecoveryItemV1 {
    pub kind: ProductionRecoveryArtifactKindV1,
    pub canonical_id: String,
    pub artifact_id: Option<String>,
    pub logical_uri: Option<String>,
    pub state: ProductionRecoveryArtifactStateV1,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProductionRecoveryViewV1 {
    pub project_id: String,
    pub items: Vec<ProductionRecoveryItemV1>,
    pub ready_for_rebuild: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProductionRecoveryRebuildOutcomeV1 {
    pub assembly: CreatorProductionPackOutcomeV1,
    pub export: ProductionPackageExportOutcomeV1,
}

pub fn inspect_creator_production_recovery_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
) -> Result<ProductionRecoveryViewV1> {
    state_store.get_project(project_id)?;
    let creator = load_latest_creator_content_scene_v1(state_store, artifact_store, project_id)?
        .ok_or_else(|| {
            Error::InvalidContract(
                "Production recovery requires verified canonical Content + ScenePlan".to_owned(),
            )
        })?;

    let mut items = Vec::new();
    for scene in &creator.scene_plan.scenes {
        let selected = latest_selected_job_artifact_v1(
            state_store,
            project_id,
            CREATOR_STEP_VISUAL_PREPARE_V1,
            &scene.id,
        )?;
        items.push(classify_artifact_v1(
            artifact_store,
            project_id,
            &scene.id,
            ProductionRecoveryArtifactKindV1::Visual,
            selected,
            Some(&["image", "video"]),
        ));
    }

    for segment in &creator.content.segments {
        let selected = latest_selected_voice_take_v1(state_store, project_id, &segment.id)?;
        match selected {
            Some((job, take)) => {
                items.push(classify_voice_member_v1(
                    artifact_store,
                    project_id,
                    &segment.id,
                    ProductionRecoveryArtifactKindV1::Audio,
                    take.artifact.clone(),
                    VOICE_AUDIO_ARTIFACT_TYPE_V1,
                    &job,
                ));
                let mut timing = classify_voice_member_v1(
                    artifact_store,
                    project_id,
                    &segment.id,
                    ProductionRecoveryArtifactKindV1::Timing,
                    take.timing_artifact.clone(),
                    VOICE_TIMING_ARTIFACT_TYPE_V1,
                    &job,
                );
                if timing.state == ProductionRecoveryArtifactStateV1::Verified {
                    match artifact_store.load_voice_timing_v1(state_store, &take.attempt.attempt_id)
                    {
                        Ok(Some(value)) if value.segment_id == segment.id => {}
                        Ok(_) => {
                            timing.state = ProductionRecoveryArtifactStateV1::Invalid;
                            timing.detail = Some(
                                "Timing sidecar does not resolve to the canonical segment identity."
                                    .to_owned(),
                            );
                        }
                        Err(_) => {
                            timing.state = ProductionRecoveryArtifactStateV1::Invalid;
                            timing.detail = Some(
                                "Timing sidecar failed canonical parsing or identity validation."
                                    .to_owned(),
                            );
                        }
                    }
                }
                items.push(timing);
            }
            None => {
                items.push(unselected_item_v1(
                    ProductionRecoveryArtifactKindV1::Audio,
                    &segment.id,
                ));
                items.push(unselected_item_v1(
                    ProductionRecoveryArtifactKindV1::Timing,
                    &segment.id,
                ));
            }
        }
    }

    let ready_for_rebuild = !items.is_empty()
        && items
            .iter()
            .all(|item| item.state == ProductionRecoveryArtifactStateV1::Verified);
    Ok(ProductionRecoveryViewV1 {
        project_id: project_id.to_owned(),
        items,
        ready_for_rebuild,
    })
}

pub fn repair_creator_production_visual_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    scene_id: &str,
    source_path: impl AsRef<Path>,
) -> Result<ManualCreatorVisualOutcomeV1> {
    require_scene_identity_v1(state_store, artifact_store, project_id, scene_id)?;
    provide_manual_creator_visual_file_v1(
        state_store,
        artifact_store,
        project_id,
        scene_id,
        source_path,
        ManualResultProvenanceV1::local_file(),
        true,
    )
}

pub fn repair_creator_production_audio_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    segment_id: &str,
    audio_path: impl AsRef<Path>,
) -> Result<ManualCreatorVoiceOutcomeV1> {
    require_segment_identity_v1(state_store, artifact_store, project_id, segment_id)?;
    let (_, take) = latest_selected_voice_take_v1(state_store, project_id, segment_id)?
        .ok_or_else(|| {
            Error::InvalidArtifact(format!(
                "segment {segment_id} has no selected voice take whose timing can be preserved"
            ))
        })?;
    let timing = artifact_store
        .load_voice_timing_v1(state_store, &take.attempt.attempt_id)?
        .ok_or_else(|| {
            Error::InvalidArtifact(format!(
                "segment {segment_id} has no verified timing sidecar; repair audio + timing together"
            ))
        })?;
    provide_manual_creator_voice_bundle_v1(
        state_store,
        artifact_store,
        ManualCreatorVoiceRequestV1 {
            project_id,
            segment_id,
            audio_path: audio_path.as_ref(),
            timing,
            provenance: ManualResultProvenanceV1::local_file(),
            replace_existing: true,
        },
    )
}

pub fn repair_creator_production_timing_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    segment_id: &str,
    timing_path: impl AsRef<Path>,
) -> Result<ManualCreatorVoiceOutcomeV1> {
    require_segment_identity_v1(state_store, artifact_store, project_id, segment_id)?;
    let (_, take) = latest_selected_voice_take_v1(state_store, project_id, segment_id)?
        .ok_or_else(|| {
            Error::InvalidArtifact(format!(
                "segment {segment_id} has no selected voice take whose audio can be preserved"
            ))
        })?;
    let audio = take.artifact.ok_or_else(|| {
        Error::InvalidArtifact(format!(
            "segment {segment_id} selected take has no audio artifact"
        ))
    })?;
    if !artifact_store.verify_artifact(&audio)? {
        return Err(Error::InvalidArtifact(format!(
            "segment {segment_id} audio is missing; repair audio + timing together"
        )));
    }
    let audio_path = artifact_store.resolve_artifact_path(&audio)?;
    let audio_metadata = inspect_manual_voice_audio_v1(&audio_path)?;
    let timing =
        import_manual_voice_timing_file_v1(timing_path, segment_id, audio_metadata.duration_ms)?;
    provide_manual_creator_voice_bundle_v1(
        state_store,
        artifact_store,
        ManualCreatorVoiceRequestV1 {
            project_id,
            segment_id,
            audio_path: &audio_path,
            timing,
            provenance: ManualResultProvenanceV1::local_file(),
            replace_existing: true,
        },
    )
}

pub fn repair_creator_production_voice_bundle_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    segment_id: &str,
    audio_path: impl AsRef<Path>,
    timing_path: impl AsRef<Path>,
) -> Result<ManualCreatorVoiceOutcomeV1> {
    require_segment_identity_v1(state_store, artifact_store, project_id, segment_id)?;
    let audio_path = audio_path.as_ref();
    let audio_metadata = inspect_manual_voice_audio_v1(audio_path)?;
    let timing =
        import_manual_voice_timing_file_v1(timing_path, segment_id, audio_metadata.duration_ms)?;
    provide_manual_creator_voice_bundle_v1(
        state_store,
        artifact_store,
        ManualCreatorVoiceRequestV1 {
            project_id,
            segment_id,
            audio_path,
            timing,
            provenance: ManualResultProvenanceV1::local_file(),
            replace_existing: true,
        },
    )
}

pub fn rebuild_and_export_creator_production_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
) -> Result<ProductionRecoveryRebuildOutcomeV1> {
    let recovery = inspect_creator_production_recovery_v1(state_store, artifact_store, project_id)?;
    if !recovery.ready_for_rebuild {
        return Err(Error::InvalidArtifact(
            "ProductionPack cannot rebuild until every canonical visual/audio/timing artifact is verified"
                .to_owned(),
        ));
    }
    let assembly = assemble_creator_production_pack_v1(
        state_store,
        artifact_store,
        project_id,
        &CreatorProductionPackOptionsV1::default(),
    )?;
    let export = ProductionPackageExporterV1::new(FcpxmlExportProfileV1::default()).export_v1(
        state_store,
        artifact_store,
        &assembly.production_pack,
    )?;
    Ok(ProductionRecoveryRebuildOutcomeV1 { assembly, export })
}

fn classify_artifact_v1(
    artifact_store: &ArtifactStore,
    project_id: &str,
    canonical_id: &str,
    kind: ProductionRecoveryArtifactKindV1,
    selected: Option<(Job, Artifact)>,
    allowed_types: Option<&[&str]>,
) -> ProductionRecoveryItemV1 {
    let Some((job, artifact)) = selected else {
        return unselected_item_v1(kind, canonical_id);
    };
    let identity_ok = artifact.project_id.as_deref() == Some(project_id)
        && artifact.producer_job.as_deref() == Some(job.job_id.as_str())
        && artifact.input_hash.as_deref() == Some(job.input_hash.as_str())
        && match allowed_types {
            Some(types) => types.contains(&artifact.artifact_type.as_str()),
            None => true,
        };
    let (state, detail) = if !identity_ok {
        (
            ProductionRecoveryArtifactStateV1::Invalid,
            Some("Artifact linkage does not match the canonical project/job identity.".to_owned()),
        )
    } else {
        match artifact_store.verify_artifact(&artifact) {
            Ok(true) => (ProductionRecoveryArtifactStateV1::Verified, None),
            Ok(false) => (
                ProductionRecoveryArtifactStateV1::Missing,
                Some(
                    "Canonical artifact file is missing at the current Data Root binding."
                        .to_owned(),
                ),
            ),
            Err(_) => (
                ProductionRecoveryArtifactStateV1::Invalid,
                Some(
                    "Canonical artifact file no longer matches its recorded hash/metadata."
                        .to_owned(),
                ),
            ),
        }
    };
    ProductionRecoveryItemV1 {
        kind,
        canonical_id: canonical_id.to_owned(),
        artifact_id: Some(artifact.artifact_id),
        logical_uri: Some(artifact.uri.to_string()),
        state,
        detail,
    }
}

fn classify_voice_member_v1(
    artifact_store: &ArtifactStore,
    project_id: &str,
    segment_id: &str,
    kind: ProductionRecoveryArtifactKindV1,
    artifact: Option<Artifact>,
    expected_type: &str,
    job: &Job,
) -> ProductionRecoveryItemV1 {
    let Some(artifact) = artifact else {
        return unselected_item_v1(kind, segment_id);
    };
    classify_artifact_v1(
        artifact_store,
        project_id,
        segment_id,
        kind,
        Some((job.clone(), artifact)),
        Some(&[expected_type]),
    )
}

fn unselected_item_v1(
    kind: ProductionRecoveryArtifactKindV1,
    canonical_id: &str,
) -> ProductionRecoveryItemV1 {
    ProductionRecoveryItemV1 {
        kind,
        canonical_id: canonical_id.to_owned(),
        artifact_id: None,
        logical_uri: None,
        state: ProductionRecoveryArtifactStateV1::Unselected,
        detail: Some("No canonical selected artifact exists for this identity.".to_owned()),
    }
}

fn latest_selected_job_artifact_v1(
    state_store: &StateStore,
    project_id: &str,
    step: &str,
    unit: &str,
) -> Result<Option<(Job, Artifact)>> {
    let mut candidates = Vec::new();
    for job in state_store.list_project_jobs(project_id)? {
        if job.step != step || job.unit != unit || job.status != StepStatus::Succeeded {
            continue;
        }
        let Some(artifact_id) = job.selected_artifact.as_deref() else {
            continue;
        };
        let artifact = state_store.get_artifact(artifact_id)?;
        candidates.push((
            artifact.created_at,
            artifact.artifact_id.clone(),
            job,
            artifact,
        ));
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    Ok(candidates
        .pop()
        .map(|(_, _, job, artifact)| (job, artifact)))
}

fn latest_selected_voice_take_v1(
    state_store: &StateStore,
    project_id: &str,
    segment_id: &str,
) -> Result<Option<(Job, VoiceTakeV1)>> {
    let mut candidates = Vec::new();
    for job in state_store.list_project_jobs(project_id)? {
        if job.step != CREATOR_TTS_STEP_V1
            || job.unit != segment_id
            || job.status != StepStatus::Succeeded
        {
            continue;
        }
        let Some(attempt_id) = job.selected_attempt.as_deref() else {
            continue;
        };
        let Some(take) = state_store.get_voice_take_v1(attempt_id)? else {
            continue;
        };
        if !take.selected || take.attempt.status != StepStatus::Succeeded {
            continue;
        }
        let created_at = take
            .artifact
            .as_ref()
            .or(take.timing_artifact.as_ref())
            .map(|artifact| artifact.created_at)
            .unwrap_or(take.attempt.started_at);
        candidates.push((created_at, take.attempt.attempt_id.clone(), job, take));
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    Ok(candidates.pop().map(|(_, _, job, take)| (job, take)))
}

fn require_scene_identity_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    scene_id: &str,
) -> Result<()> {
    let creator = load_latest_creator_content_scene_v1(state_store, artifact_store, project_id)?
        .ok_or_else(|| {
            Error::InvalidContract(
                "Production recovery requires verified canonical Content + ScenePlan".to_owned(),
            )
        })?;
    if creator
        .scene_plan
        .scenes
        .iter()
        .any(|scene| scene.id == scene_id)
    {
        Ok(())
    } else {
        Err(Error::InvalidContract(format!(
            "recovery scene {scene_id} is not a canonical scene identity"
        )))
    }
}

fn require_segment_identity_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    segment_id: &str,
) -> Result<()> {
    let creator = load_latest_creator_content_scene_v1(state_store, artifact_store, project_id)?
        .ok_or_else(|| {
            Error::InvalidContract(
                "Production recovery requires verified canonical Content + ScenePlan".to_owned(),
            )
        })?;
    if creator
        .content
        .segments
        .iter()
        .any(|segment| segment.id == segment_id)
    {
        Ok(())
    } else {
        Err(Error::InvalidContract(format!(
            "recovery segment {segment_id} is not a canonical segment identity"
        )))
    }
}
