use std::{fs, path::PathBuf};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    artifact_store::{AttemptOutputPromotion, AttemptPromotionRequest},
    deterministic_input_hash, Artifact, ArtifactStore, CreatorContentV1, CreatorScenePlanV1, Error,
    Job, LogicalUri, ProductionPackV1, Result, StateStore, StepStatus, SubtitleCueV1,
    TimelineClipV1, TimelineFrameRateV1, TimelineMarkerKindV1, TimelineMarkerV1, TimelineTrackRoleV1,
    TimelineTrackV1, WorkflowStep, CREATOR_CONTENT_ARTIFACT_TYPE_V1,
    CREATOR_SCENE_PLAN_ARTIFACT_TYPE_V1, CREATOR_STEP_CONTENT_PREPARE_V1,
    CREATOR_STEP_PRODUCTION_PACK_V1, CREATOR_STEP_SCENE_PLAN_V1,
    CREATOR_STEP_VISUAL_PREPARE_V1, CREATOR_STEP_VOICE_PREPARE_V1, CREATOR_TTS_STEP_V1,
    CREATOR_WORKFLOW_UNIT_PROJECT_V1,
};

pub const CREATOR_PRODUCTION_PACK_ARTIFACT_TYPE_V1: &str = "production_pack";
pub const CREATOR_PRODUCTION_ASSEMBLER_VERSION_V1: &str = "phase15-p4-v1";
const CREATOR_PRODUCTION_WORKER_V1: &str = "creator-production-assembler-v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreatorProductionPackOptionsV1 {
    pub frame_rate: TimelineFrameRateV1,
}

impl Default for CreatorProductionPackOptionsV1 {
    fn default() -> Self {
        Self {
            frame_rate: TimelineFrameRateV1 {
                numerator: 24,
                denominator: 1,
            },
        }
    }
}

impl CreatorProductionPackOptionsV1 {
    pub fn validate_v1(&self) -> Result<()> {
        self.frame_rate.validate_v1()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreatorProductionPackOutcomeV1 {
    pub production_pack: ProductionPackV1,
    pub artifact: Artifact,
    pub input_hash: String,
    pub cache_hit: bool,
    pub total_duration_ms: u64,
}

#[derive(Debug, Clone)]
struct SelectedVoiceSegmentV1 {
    audio: Artifact,
    timing_artifact: Artifact,
    timing: crate::VoiceTimingV1,
}

pub fn assemble_creator_production_pack_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    options: &CreatorProductionPackOptionsV1,
) -> Result<CreatorProductionPackOutcomeV1> {
    options.validate_v1()?;
    let project = state_store.get_project(project_id)?;
    if !matches!(project.studio_pack.as_deref(), Some(value) if !value.is_empty()) {
        return Err(Error::InvalidContract(
            "creator ProductionPack assembly requires a Project bound to a Studio Pack".to_owned(),
        ));
    }

    let steps = state_store.list_project_steps(project_id)?;
    let content_step = require_project_step_v1(&steps, CREATOR_STEP_CONTENT_PREPARE_V1)?;
    let scene_step = require_project_step_v1(&steps, CREATOR_STEP_SCENE_PLAN_V1)?;
    let visual_step = require_project_step_v1(&steps, CREATOR_STEP_VISUAL_PREPARE_V1)?;
    let voice_step = require_project_step_v1(&steps, CREATOR_STEP_VOICE_PREPARE_V1)?;
    let mut production_step = require_project_step_v1(&steps, CREATOR_STEP_PRODUCTION_PACK_V1)?;

    for (label, step) in [
        (CREATOR_STEP_CONTENT_PREPARE_V1, &content_step),
        (CREATOR_STEP_SCENE_PLAN_V1, &scene_step),
        (CREATOR_STEP_VISUAL_PREPARE_V1, &visual_step),
        (CREATOR_STEP_VOICE_PREPARE_V1, &voice_step),
    ] {
        if step.status != StepStatus::Succeeded {
            return Err(Error::InvalidJobState(format!(
                "{label}/{} must be SUCCEEDED before ProductionPack assembly; found {}",
                CREATOR_WORKFLOW_UNIT_PROJECT_V1,
                step.status.as_str()
            )));
        }
    }

    let (content, content_artifact) = load_selected_stage_json_v1::<CreatorContentV1>(
        state_store,
        artifact_store,
        project_id,
        CREATOR_STEP_CONTENT_PREPARE_V1,
        CREATOR_CONTENT_ARTIFACT_TYPE_V1,
    )?;
    content.validate_v1()?;
    if content.project_id != project.id {
        return Err(Error::InvalidContract(
            "creator content Project does not match ProductionPack Project".to_owned(),
        ));
    }

    let (scene_plan, scene_artifact) = load_selected_stage_json_v1::<CreatorScenePlanV1>(
        state_store,
        artifact_store,
        project_id,
        CREATOR_STEP_SCENE_PLAN_V1,
        CREATOR_SCENE_PLAN_ARTIFACT_TYPE_V1,
    )?;
    scene_plan.validate_v1(&content)?;

    let mut visual_clips = Vec::with_capacity(content.segments.len());
    let mut narration_clips = Vec::with_capacity(content.segments.len());
    let mut subtitles = Vec::new();
    let mut markers = Vec::with_capacity(content.segments.len());
    let mut selected_artifacts = vec![content_artifact, scene_artifact];
    let mut timeline_start_ms = 0_u64;

    for (segment, scene) in content.segments.iter().zip(&scene_plan.scenes) {
        if scene.segment_id != segment.id {
            return Err(Error::InvalidContract(format!(
                "scene {} does not map to creator segment {}",
                scene.id, segment.id
            )));
        }

        let visual =
            selected_verified_job_artifact_v1(state_store, artifact_store, project_id, CREATOR_STEP_VISUAL_PREPARE_V1, &scene.id)?;
        let voice = selected_voice_segment_v1(
            state_store,
            artifact_store,
            project_id,
            &segment.id,
        )?;
        let duration_ms = voice.timing.duration_ms;

        visual_clips.push(TimelineClipV1 {
            clip_id: format!("visual-{}", scene.id),
            artifact_id: visual.artifact_id.clone(),
            uri: visual.uri.clone(),
            timeline_start_ms,
            source_start_ms: 0,
            duration_ms,
            label: Some(scene.purpose.clone()),
        });
        narration_clips.push(TimelineClipV1 {
            clip_id: format!("voice-{}", segment.id),
            artifact_id: voice.audio.artifact_id.clone(),
            uri: voice.audio.uri.clone(),
            timeline_start_ms,
            source_start_ms: 0,
            duration_ms,
            label: Some(segment.id.clone()),
        });

        for cue in &voice.timing.cues {
            let start_ms = timeline_start_ms.checked_add(cue.start_ms).ok_or_else(|| {
                Error::InvalidContract("subtitle start time overflowed u64 milliseconds".to_owned())
            })?;
            let end_ms = timeline_start_ms.checked_add(cue.end_ms).ok_or_else(|| {
                Error::InvalidContract("subtitle end time overflowed u64 milliseconds".to_owned())
            })?;
            subtitles.push(SubtitleCueV1 {
                cue_id: format!("{}-C{:03}", segment.id, cue.index + 1),
                start_ms,
                end_ms,
                text: cue.text.clone(),
            });
        }

        markers.push(TimelineMarkerV1 {
            marker_id: format!("scene-{}", scene.id),
            position_ms: timeline_start_ms,
            label: scene.purpose.clone(),
            kind: TimelineMarkerKindV1::Scene,
        });

        timeline_start_ms = timeline_start_ms.checked_add(duration_ms).ok_or_else(|| {
            Error::InvalidContract("ProductionPack duration overflowed u64 milliseconds".to_owned())
        })?;
        selected_artifacts.push(visual);
        selected_artifacts.push(voice.audio);
        selected_artifacts.push(voice.timing_artifact);
    }

    let production_pack = ProductionPackV1 {
        schema: crate::PRODUCTION_PACK_SCHEMA_V1.to_owned(),
        version: crate::PRODUCTION_PACK_VERSION_V1,
        project_id: project.id.clone(),
        title: project.title.clone(),
        frame_rate: options.frame_rate,
        tracks: vec![
            TimelineTrackV1 {
                role: TimelineTrackRoleV1::VideoPrimary,
                clips: visual_clips,
            },
            TimelineTrackV1 {
                role: TimelineTrackRoleV1::AudioNarration,
                clips: narration_clips,
            },
        ],
        subtitles,
        markers,
    }
    .normalized_v1()?;

    let pack_json = serde_json::to_vec(&production_pack)?;
    let selected_fingerprint = selected_artifacts
        .iter()
        .map(|artifact| format!("{}:{}", artifact.artifact_id, artifact.sha256))
        .collect::<Vec<_>>()
        .join("\n");
    let input_hash = deterministic_input_hash(&[
        CREATOR_PRODUCTION_ASSEMBLER_VERSION_V1.as_bytes(),
        selected_fingerprint.as_bytes(),
        &pack_json,
    ]);

    if let Some((artifact, cached_pack)) = find_verified_assembly_cache_v1(
        state_store,
        artifact_store,
        project_id,
        &input_hash,
    )? {
        if cached_pack != production_pack {
            return Err(Error::InvalidArtifact(
                "cached creator ProductionPack does not match deterministic assembly".to_owned(),
            ));
        }
        mark_production_step_succeeded_v1(state_store, &production_step.step_id)?;
        return Ok(CreatorProductionPackOutcomeV1 {
            production_pack: cached_pack,
            artifact,
            input_hash,
            cache_hit: true,
            total_duration_ms: timeline_start_ms,
        });
    }

    production_step = prepare_production_step_v1(state_store, &production_step)?;
    let job = get_or_create_production_job_v1(state_store, project_id, &input_hash)?;
    state_store.set_step_status(&production_step.step_id, StepStatus::Running)?;
    let attempt = state_store.start_attempt(&job.job_id, Some(CREATOR_PRODUCTION_WORKER_V1))?;

    let staging = write_staging_pack_v1(artifact_store, &production_pack)?;
    let target_uri = LogicalUri::parse(&format!(
        "project://production/assembly/{}/production-pack.json",
        &input_hash[..16]
    ))?;
    let promotion = artifact_store.promote_attempt_outputs(
        state_store,
        AttemptPromotionRequest {
            attempt_id: attempt.attempt_id.clone(),
            job_id: job.job_id.clone(),
            outputs: vec![AttemptOutputPromotion {
                source: staging.clone(),
                target_uri,
                artifact_type: CREATOR_PRODUCTION_PACK_ARTIFACT_TYPE_V1.to_owned(),
                metadata: serde_json::json!({
                    "schema": crate::PRODUCTION_PACK_SCHEMA_V1,
                    "version": crate::PRODUCTION_PACK_VERSION_V1,
                    "stage": CREATOR_STEP_PRODUCTION_PACK_V1,
                    "assembler": CREATOR_PRODUCTION_ASSEMBLER_VERSION_V1,
                    "total_duration_ms": timeline_start_ms,
                    "source_artifact_ids": selected_artifacts
                        .iter()
                        .map(|artifact| artifact.artifact_id.clone())
                        .collect::<Vec<_>>(),
                }),
                expected_sha256: None,
            }],
            selected_output_index: 0,
        },
    );
    let _ = fs::remove_file(&staging);
    cleanup_empty_parent_v1(staging.parent());

    let artifact = match promotion {
        Ok(mut artifacts) => artifacts.pop().ok_or_else(|| {
            Error::InvalidArtifact(
                "creator ProductionPack assembly produced no committed artifact".to_owned(),
            )
        })?,
        Err(error) => {
            let _ = state_store
                .finish_attempt_failure(&attempt.attempt_id, "LOCAL_EXPORT_ERROR");
            let failed = state_store.get_attempt(&attempt.attempt_id)?;
            if state_store.get_step(&production_step.step_id)?.status == StepStatus::Running {
                state_store.set_step_status(&production_step.step_id, failed.status)?;
            }
            return Err(error);
        }
    };

    state_store.set_step_status(&production_step.step_id, StepStatus::Succeeded)?;
    state_store.refresh_ready_steps(project_id)?;

    Ok(CreatorProductionPackOutcomeV1 {
        production_pack,
        artifact,
        input_hash,
        cache_hit: false,
        total_duration_ms: timeline_start_ms,
    })
}

pub fn load_latest_creator_production_pack_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
) -> Result<Option<CreatorProductionPackOutcomeV1>> {
    state_store.get_project(project_id)?;
    let mut candidates = Vec::new();
    for job in state_store.list_project_jobs(project_id)? {
        if job.step != CREATOR_STEP_PRODUCTION_PACK_V1
            || job.unit != CREATOR_WORKFLOW_UNIT_PROJECT_V1
            || job.status != StepStatus::Succeeded
        {
            continue;
        }
        let Some(artifact_id) = job.selected_artifact.as_deref() else {
            continue;
        };
        let artifact = state_store.get_artifact(artifact_id)?;
        if artifact.artifact_type != CREATOR_PRODUCTION_PACK_ARTIFACT_TYPE_V1
            || !artifact_store.verify_artifact(&artifact)?
        {
            continue;
        }
        let pack: ProductionPackV1 = read_json_artifact_v1(artifact_store, &artifact)?;
        let pack = pack.normalized_v1()?;
        if pack.project_id != project_id {
            return Err(Error::InvalidArtifact(
                "persisted creator ProductionPack belongs to a different Project".to_owned(),
            ));
        }
        let duration_ms = pack
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .map(TimelineClipV1::timeline_end_ms_v1)
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .max()
            .unwrap_or(0);
        candidates.push((
            artifact.created_at,
            artifact.artifact_id.clone(),
            CreatorProductionPackOutcomeV1 {
                production_pack: pack,
                artifact,
                input_hash: job.input_hash,
                cache_hit: true,
                total_duration_ms: duration_ms,
            },
        ));
    }
    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    Ok(candidates.pop().map(|(_, _, outcome)| outcome))
}

fn require_project_step_v1(steps: &[WorkflowStep], key: &str) -> Result<WorkflowStep> {
    steps
        .iter()
        .find(|step| step.step == key && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1)
        .cloned()
        .ok_or_else(|| {
            Error::InvalidContract(format!(
                "creator workflow step {key}/{} is missing",
                CREATOR_WORKFLOW_UNIT_PROJECT_V1
            ))
        })
}

fn load_selected_stage_json_v1<T: DeserializeOwned>(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    step_key: &str,
    artifact_type: &str,
) -> Result<(T, Artifact)> {
    let artifact = selected_verified_job_artifact_v1(
        state_store,
        artifact_store,
        project_id,
        step_key,
        CREATOR_WORKFLOW_UNIT_PROJECT_V1,
    )?;
    if artifact.artifact_type != artifact_type {
        return Err(Error::InvalidArtifact(format!(
            "selected {step_key} artifact type {} does not match expected {artifact_type}",
            artifact.artifact_type
        )));
    }
    let value = read_json_artifact_v1(artifact_store, &artifact)?;
    Ok((value, artifact))
}

fn selected_verified_job_artifact_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    step_key: &str,
    unit: &str,
) -> Result<Artifact> {
    let mut candidates = Vec::new();
    for job in state_store.list_project_jobs(project_id)? {
        if job.step != step_key || job.unit != unit || job.status != StepStatus::Succeeded {
            continue;
        }
        let Some(artifact_id) = job.selected_artifact.as_deref() else {
            continue;
        };
        let artifact = state_store.get_artifact(artifact_id)?;
        if artifact.project_id.as_deref() != Some(project_id)
            || artifact.producer_job.as_deref() != Some(job.job_id.as_str())
            || artifact.input_hash.as_deref() != Some(job.input_hash.as_str())
            || !artifact_store.verify_artifact(&artifact)?
        {
            continue;
        }
        candidates.push((artifact.created_at, artifact.artifact_id.clone(), artifact));
    }
    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    candidates.pop().map(|(_, _, artifact)| artifact).ok_or_else(|| {
        Error::InvalidArtifact(format!(
            "no selected verified artifact exists for {step_key}/{unit}"
        ))
    })
}

fn selected_voice_segment_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    segment_id: &str,
) -> Result<SelectedVoiceSegmentV1> {
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
        let Some(audio) = take.artifact else {
            continue;
        };
        let Some(timing_artifact) = take.timing_artifact else {
            continue;
        };
        if !take.selected
            || audio.project_id.as_deref() != Some(project_id)
            || !artifact_store.verify_artifact(&audio)?
            || !artifact_store.verify_artifact(&timing_artifact)?
        {
            continue;
        }
        let timing = artifact_store
            .load_voice_timing_v1(state_store, attempt_id)?
            .ok_or_else(|| {
                Error::InvalidArtifact(format!(
                    "selected voice take for {segment_id} has no valid timing sidecar"
                ))
            })?;
        if timing.segment_id != segment_id {
            return Err(Error::InvalidArtifact(format!(
                "voice timing {} does not match segment {segment_id}",
                timing.segment_id
            )));
        }
        candidates.push((
            audio.created_at,
            audio.artifact_id.clone(),
            SelectedVoiceSegmentV1 {
                audio,
                timing_artifact,
                timing,
            },
        ));
    }
    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    candidates.pop().map(|(_, _, selected)| selected).ok_or_else(|| {
        Error::InvalidArtifact(format!(
            "no selected verified voice+timing bundle exists for segment {segment_id}"
        ))
    })
}

fn find_verified_assembly_cache_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    input_hash: &str,
) -> Result<Option<(Artifact, ProductionPackV1)>> {
    for job in state_store.list_project_jobs(project_id)? {
        if job.step != CREATOR_STEP_PRODUCTION_PACK_V1
            || job.unit != CREATOR_WORKFLOW_UNIT_PROJECT_V1
            || job.input_hash != input_hash
            || job.status != StepStatus::Succeeded
        {
            continue;
        }
        let Some(artifact_id) = job.selected_artifact.as_deref() else {
            continue;
        };
        let artifact = state_store.get_artifact(artifact_id)?;
        if artifact.artifact_type != CREATOR_PRODUCTION_PACK_ARTIFACT_TYPE_V1
            || !artifact_store.verify_artifact(&artifact)?
        {
            continue;
        }
        let pack = read_json_artifact_v1::<ProductionPackV1>(artifact_store, &artifact)?
            .normalized_v1()?;
        return Ok(Some((artifact, pack)));
    }
    Ok(None)
}

fn prepare_production_step_v1(
    state_store: &mut StateStore,
    step: &WorkflowStep,
) -> Result<WorkflowStep> {
    state_store.refresh_ready_steps(&step.project_id)?;
    let current = state_store.get_step(&step.step_id)?;
    match current.status {
        StepStatus::Ready => Ok(current),
        StepStatus::Succeeded => {
            state_store.invalidate_from(&current.step_id, None)?;
            state_store.set_step_status(&current.step_id, StepStatus::Ready)?;
            state_store.get_step(&current.step_id)
        }
        StepStatus::Stale
        | StepStatus::Retryable
        | StepStatus::Failed
        | StepStatus::Skipped
        | StepStatus::Cancelled => {
            state_store.set_step_status(&current.step_id, StepStatus::Ready)?;
            state_store.get_step(&current.step_id)
        }
        StepStatus::NotReady => Err(Error::InvalidJobState(
            "production.pack/project is still waiting on visual/voice dependencies".to_owned(),
        )),
        StepStatus::Running | StepStatus::Queued => Err(Error::InvalidJobState(
            "production.pack/project is already active".to_owned(),
        )),
        StepStatus::Fatal => Err(Error::InvalidJobState(
            "production.pack/project is FATAL".to_owned(),
        )),
    }
}

fn mark_production_step_succeeded_v1(state_store: &mut StateStore, step_id: &str) -> Result<()> {
    let current = state_store.get_step(step_id)?;
    if current.status == StepStatus::Succeeded {
        return Ok(());
    }
    if current.status == StepStatus::NotReady {
        state_store.refresh_ready_steps(&current.project_id)?;
    }
    let current = state_store.get_step(step_id)?;
    if current.status == StepStatus::Stale {
        state_store.set_step_status(step_id, StepStatus::Ready)?;
    }
    let current = state_store.get_step(step_id)?;
    if current.status == StepStatus::Ready || current.status == StepStatus::Retryable {
        state_store.set_step_status(step_id, StepStatus::Succeeded)?;
        state_store.refresh_ready_steps(&current.project_id)?;
        return Ok(());
    }
    Err(Error::InvalidJobState(format!(
        "production.pack/project cannot be marked SUCCEEDED from {}",
        current.status.as_str()
    )))
}

fn get_or_create_production_job_v1(
    state_store: &mut StateStore,
    project_id: &str,
    input_hash: &str,
) -> Result<Job> {
    let matching = state_store
        .list_project_jobs(project_id)?
        .into_iter()
        .find(|job| {
            job.step == CREATOR_STEP_PRODUCTION_PACK_V1
                && job.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1
                && job.input_hash == input_hash
        });
    match matching {
        Some(job) if matches!(job.status, StepStatus::Retryable | StepStatus::Failed) => {
            state_store.prepare_job_retry(&job.job_id)
        }
        Some(job) if job.status == StepStatus::Ready => Ok(job),
        Some(job) if matches!(job.status, StepStatus::Running | StepStatus::Queued) => {
            Err(Error::InvalidJobState(
                "creator ProductionPack assembly job is already active".to_owned(),
            ))
        }
        Some(job) if job.status == StepStatus::Fatal => Err(Error::InvalidJobState(
            "creator ProductionPack assembly job is FATAL for unchanged input".to_owned(),
        )),
        _ => state_store.create_job(
            project_id,
            CREATOR_STEP_PRODUCTION_PACK_V1,
            CREATOR_WORKFLOW_UNIT_PROJECT_V1,
            input_hash,
        ),
    }
}

fn read_json_artifact_v1<T: DeserializeOwned>(
    artifact_store: &ArtifactStore,
    artifact: &Artifact,
) -> Result<T> {
    if !artifact_store.verify_artifact(artifact)? {
        return Err(Error::ArtifactNotFound(artifact.artifact_id.clone()));
    }
    let path = artifact_store.resolve_artifact_path(artifact)?;
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_staging_pack_v1(
    artifact_store: &ArtifactStore,
    production_pack: &ProductionPackV1,
) -> Result<PathBuf> {
    let directory = artifact_store
        .data_root()
        .join("cache")
        .join("creator-orchestration");
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!(
        "production-pack-{}.json",
        Uuid::new_v4().simple()
    ));
    fs::write(&path, serde_json::to_vec_pretty(production_pack)?)?;
    Ok(path)
}

fn cleanup_empty_parent_v1(parent: Option<&std::path::Path>) {
    if let Some(parent) = parent {
        let _ = fs::remove_dir(parent);
    }
}
