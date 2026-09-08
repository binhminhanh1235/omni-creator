use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    deterministic_input_hash, fs_util::sha256_file, load_latest_creator_content_v1, Artifact,
    ArtifactStore, CreatorContentV1, CreatorSegmentV1, Error, Job, LogicalUri,
    ManualResultProducerV1, ManualResultProvenanceV1, PathResolver, Result, StateStore, StepStatus,
    VoiceTimingCueV1, VoiceTimingV1, WorkflowStep, CREATOR_STEP_CONTENT_PREPARE_V1,
    CREATOR_STEP_PRODUCTION_PACK_V1, CREATOR_STEP_VOICE_PREPARE_V1, CREATOR_TTS_STEP_V1,
    CREATOR_WORKFLOW_UNIT_PROJECT_V1, VOICE_AUDIO_ARTIFACT_TYPE_V1, VOICE_TIMING_ARTIFACT_TYPE_V1,
    VOICE_TIMING_SCHEMA_V1,
};

pub const MANUAL_VOICE_SCHEMA_V1: &str = "omnicreator.manual-voice";
pub const MANUAL_VOICE_VERSION_V1: u32 = 1;
pub const MAX_MANUAL_VOICE_AUDIO_BYTES_V1: u64 = 512 * 1024 * 1024;
pub const MAX_MANUAL_VOICE_TIMING_BYTES_V1: u64 = 8 * 1024 * 1024;
const AUDIO_DURATION_TOLERANCE_MS_V1: u64 = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManualVoiceAudioMetadataV1 {
    pub schema: String,
    pub version: u32,
    pub extension: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub duration_ms: Option<u64>,
}

impl ManualVoiceAudioMetadataV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != MANUAL_VOICE_SCHEMA_V1 || self.version != MANUAL_VOICE_VERSION_V1 {
            return Err(Error::InvalidContract(
                "unsupported manual voice metadata schema/version".to_owned(),
            ));
        }
        if self.extension.trim().is_empty()
            || self.mime_type.trim().is_empty()
            || self.size_bytes == 0
            || self.duration_ms.is_some_and(|value| value == 0)
        {
            return Err(Error::InvalidContract(
                "manual voice extension, mime type and non-zero sizes/duration are required"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ManualCreatorVoiceRequestV1<'a> {
    pub project_id: &'a str,
    pub segment_id: &'a str,
    pub audio_path: &'a Path,
    pub timing: VoiceTimingV1,
    pub provenance: ManualResultProvenanceV1,
    pub replace_existing: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ManualCreatorVoiceOutcomeV1 {
    pub segment_id: String,
    pub audio: Artifact,
    pub timing_artifact: Artifact,
    pub timing: VoiceTimingV1,
    pub audio_metadata: ManualVoiceAudioMetadataV1,
    pub job: Job,
    pub attempt_id: String,
    pub cache_hit: bool,
    pub invalidated_step_ids: Vec<String>,
    pub voice_stage_complete: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CreatorSegmentVoiceStateV1 {
    pub segment_id: String,
    pub audio: Option<Artifact>,
    pub timing_artifact: Option<Artifact>,
    pub timing: Option<VoiceTimingV1>,
    pub verified: bool,
}

#[derive(Debug, Clone)]
struct VerifiedSegmentVoiceV1 {
    job: Job,
    audio: Artifact,
    timing_artifact: Artifact,
    timing: VoiceTimingV1,
    attempt_id: String,
}

pub fn inspect_manual_voice_audio_v1(
    path: impl AsRef<Path>,
) -> Result<ManualVoiceAudioMetadataV1> {
    let path = path.as_ref();
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(Error::InvalidContract(
            "manual voice audio must be a non-empty regular file".to_owned(),
        ));
    }
    if metadata.len() > MAX_MANUAL_VOICE_AUDIO_BYTES_V1 {
        return Err(Error::InvalidContract(format!(
            "manual voice audio exceeds the {}-byte safety limit",
            MAX_MANUAL_VOICE_AUDIO_BYTES_V1
        )));
    }
    let extension = extension_v1(path, "manual voice audio")?;
    let prefix = fs::read(path)?;
    let (mime_type, duration_ms) = match extension.as_str() {
        "wav" => ("audio/wav".to_owned(), Some(wav_duration_ms_v1(&prefix)?)),
        "mp3" => {
            validate_mp3_v1(&prefix)?;
            ("audio/mpeg".to_owned(), None)
        }
        other => {
            return Err(Error::InvalidContract(format!(
                "manual voice extension .{other} is not supported; use WAV or MP3"
            )))
        }
    };
    let result = ManualVoiceAudioMetadataV1 {
        schema: MANUAL_VOICE_SCHEMA_V1.to_owned(),
        version: MANUAL_VOICE_VERSION_V1,
        extension,
        mime_type,
        size_bytes: metadata.len(),
        duration_ms,
    };
    result.validate_v1()?;
    Ok(result)
}

pub fn derive_manual_voice_timing_v1(
    segment_id: &str,
    narration: &str,
    duration_ms: u64,
) -> Result<VoiceTimingV1> {
    if segment_id.trim().is_empty() || narration.trim().is_empty() || duration_ms == 0 {
        return Err(Error::InvalidContract(
            "derived manual timing requires segment id, narration and positive duration".to_owned(),
        ));
    }
    let timing = VoiceTimingV1 {
        schema: VOICE_TIMING_SCHEMA_V1.to_owned(),
        version: 1,
        segment_id: segment_id.to_owned(),
        duration_ms,
        cues: vec![VoiceTimingCueV1 {
            index: 0,
            text: narration.trim().to_owned(),
            start_ms: 0,
            end_ms: duration_ms,
        }],
    };
    timing.validate_v1()?;
    Ok(timing)
}

pub fn parse_manual_voice_timing_bytes_v1(
    bytes: &[u8],
    extension: &str,
    segment_id: &str,
    audio_duration_ms: Option<u64>,
) -> Result<VoiceTimingV1> {
    if bytes.len() as u64 > MAX_MANUAL_VOICE_TIMING_BYTES_V1 {
        return Err(Error::InvalidContract(format!(
            "manual voice timing exceeds the {}-byte safety limit",
            MAX_MANUAL_VOICE_TIMING_BYTES_V1
        )));
    }
    let extension = extension.trim().to_ascii_lowercase();
    let mut timing = match extension.as_str() {
        "srt" => parse_srt_v1(bytes, segment_id, audio_duration_ms)?,
        "json" => {
            let timing = VoiceTimingV1::from_json_bytes_v1(bytes)?;
            if timing.segment_id != segment_id {
                return Err(Error::InvalidContract(format!(
                    "voice timing segment_id {} does not match target segment {segment_id}",
                    timing.segment_id
                )));
            }
            timing
        }
        other => {
            return Err(Error::InvalidContract(format!(
                "manual voice timing extension .{other} is not supported; use SRT or JSON"
            )))
        }
    };
    validate_timing_against_audio_v1(&timing, audio_duration_ms)?;
    if extension == "srt" {
        if let Some(duration_ms) = audio_duration_ms {
            timing.duration_ms = duration_ms;
            timing.validate_v1()?;
        }
    }
    Ok(timing)
}

pub fn import_manual_voice_timing_file_v1(
    path: impl AsRef<Path>,
    segment_id: &str,
    audio_duration_ms: Option<u64>,
) -> Result<VoiceTimingV1> {
    let path = path.as_ref();
    let bytes = read_limited_v1(
        path,
        MAX_MANUAL_VOICE_TIMING_BYTES_V1,
        "manual voice timing",
    )?;
    let extension = extension_v1(path, "manual voice timing")?;
    parse_manual_voice_timing_bytes_v1(&bytes, &extension, segment_id, audio_duration_ms)
}

pub fn voice_timing_to_srt_v1(timing: &VoiceTimingV1) -> Result<String> {
    timing.validate_v1()?;
    let mut output = String::new();
    for cue in &timing.cues {
        output.push_str(&(cue.index + 1).to_string());
        output.push('\n');
        output.push_str(&format!(
            "{} --> {}\n",
            format_srt_time_v1(cue.start_ms),
            format_srt_time_v1(cue.end_ms)
        ));
        output.push_str(&cue.text);
        output.push_str("\n\n");
    }
    Ok(output)
}

pub fn provide_manual_creator_voice_bundle_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    request: ManualCreatorVoiceRequestV1<'_>,
) -> Result<ManualCreatorVoiceOutcomeV1> {
    request.provenance.validate_v1()?;
    let (content, _) = load_latest_creator_content_v1(
        state_store,
        artifact_store,
        request.project_id,
    )?
    .ok_or_else(|| {
        Error::InvalidContract("manual voice requires verified creator Content".to_owned())
    })?;
    let segment = content
        .segments
        .iter()
        .find(|segment| segment.id == request.segment_id)
        .ok_or_else(|| {
            Error::InvalidContract(format!(
                "creator content segment not found: {}",
                request.segment_id
            ))
        })?;
    let audio_metadata = inspect_manual_voice_audio_v1(request.audio_path)?;
    request.timing.validate_v1()?;
    if request.timing.segment_id != segment.id {
        return Err(Error::InvalidContract(format!(
            "voice timing segment {} does not match target {}",
            request.timing.segment_id, segment.id
        )));
    }
    validate_timing_against_audio_v1(&request.timing, audio_metadata.duration_ms)?;

    let (audio_sha256, audio_size) = sha256_file(request.audio_path)?;
    let timing_bytes = request.timing.to_json_bytes_v1()?;
    let timing_sha256 = crate::hashing::sha256_bytes(&timing_bytes);
    let provenance_json = serde_json::to_vec(&request.provenance)?;
    let input_hash = deterministic_input_hash(&[
        b"manual-voice-bundle-v1",
        request.project_id.as_bytes(),
        request.segment_id.as_bytes(),
        audio_sha256.as_bytes(),
        audio_size.to_string().as_bytes(),
        timing_sha256.as_bytes(),
        provenance_json.as_slice(),
    ]);

    if let Some(existing) = verified_voice_for_input_hash_v1(
        state_store,
        artifact_store,
        request.project_id,
        request.segment_id,
        &input_hash,
    )? {
        let voice_stage_complete =
            reconcile_creator_voice_aggregate_v1(state_store, artifact_store, request.project_id)?;
        return Ok(ManualCreatorVoiceOutcomeV1 {
            segment_id: request.segment_id.to_owned(),
            audio: existing.audio,
            timing_artifact: existing.timing_artifact,
            timing: existing.timing,
            audio_metadata,
            job: existing.job,
            attempt_id: existing.attempt_id,
            cache_hit: true,
            invalidated_step_ids: Vec::new(),
            voice_stage_complete,
        });
    }

    let current = latest_verified_segment_voice_v1(
        state_store,
        artifact_store,
        request.project_id,
        request.segment_id,
    )?;
    if current.is_some() && !request.replace_existing {
        return Err(Error::InvalidJobState(format!(
            "voice segment {} already has a verified selected take; replacement must be explicit",
            request.segment_id
        )));
    }

    let (segment_step, mut invalidated_step_ids) = ensure_manual_tts_step_v1(
        state_store,
        request.project_id,
        request.segment_id,
        &input_hash,
        request.replace_existing,
    )?;
    if request.replace_existing {
        state_store.supersede_logical_job_results_v1(
            request.project_id,
            CREATOR_TTS_STEP_V1,
            request.segment_id,
            Some(&input_hash),
        )?;
    }

    let job = state_store.create_job(
        request.project_id,
        CREATOR_TTS_STEP_V1,
        request.segment_id,
        &input_hash,
    )?;
    let started = state_store.start_voice_take_attempt_v1(
        &job.job_id,
        Some(manual_voice_worker_v1(&request.provenance)),
    )?;

    let bundle_id = deterministic_input_hash(&[
        b"manual-voice-target-v1",
        request.segment_id.as_bytes(),
        input_hash.as_bytes(),
    ]);
    let audio_uri = LogicalUri::parse(&format!(
        "project://voice/manual/{}/{bundle_id}.{}",
        request.segment_id, audio_metadata.extension
    ))?;
    let timing_uri = crate::voice_timing_output_uri_v1(&audio_uri)?;
    let timing_staging = manual_voice_timing_staging_path_v1(artifact_store)?;
    if let Some(parent) = timing_staging.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&timing_staging, &timing_bytes)?;

    let promotion = artifact_store.promote_local_voice_bundle_v1(
        state_store,
        &started.attempt.attempt_id,
        request.audio_path,
        audio_uri,
        &timing_staging,
        timing_uri,
        serde_json::json!({
            "manual_result": {
                "provenance": &request.provenance,
                "job_step": CREATOR_TTS_STEP_V1,
                "job_unit": request.segment_id,
                "source_sha256": &audio_sha256,
                "source_size_bytes": audio_size,
            },
            "manual_voice": {
                "schema": MANUAL_VOICE_SCHEMA_V1,
                "version": MANUAL_VOICE_VERSION_V1,
                "segment_id": request.segment_id,
                "mime_type": &audio_metadata.mime_type,
                "extension": &audio_metadata.extension,
                "duration_ms": audio_metadata.duration_ms,
                "timing_sha256": &timing_sha256,
            }
        }),
    );
    let _ = fs::remove_file(&timing_staging);
    let (audio, timing_artifact) = match promotion {
        Ok(bundle) => (bundle.audio, bundle.timing),
        Err(error) => {
            let _ = state_store.finish_attempt_failure(
                &started.attempt.attempt_id,
                "MANUAL_VOICE_IMPORT_ERROR",
            );
            return Err(error);
        }
    };

    let current_step = state_store.get_step(&segment_step.step_id)?;
    if current_step.status == StepStatus::Stale {
        state_store.set_step_status(&current_step.step_id, StepStatus::NotReady)?;
        state_store.refresh_ready_steps(request.project_id)?;
    }
    let current_step = state_store.get_step(&segment_step.step_id)?;
    if current_step.status == StepStatus::NotReady {
        state_store.refresh_ready_steps(request.project_id)?;
    }
    let current_step = state_store.get_step(&segment_step.step_id)?;
    if matches!(current_step.status, StepStatus::Ready | StepStatus::Retryable) {
        state_store.set_step_status(&current_step.step_id, StepStatus::Succeeded)?;
    }
    if !invalidated_step_ids.contains(&segment_step.step_id) && request.replace_existing {
        invalidated_step_ids.push(segment_step.step_id.clone());
    }

    let voice_stage_complete =
        reconcile_creator_voice_aggregate_v1(state_store, artifact_store, request.project_id)?;
    Ok(ManualCreatorVoiceOutcomeV1 {
        segment_id: request.segment_id.to_owned(),
        audio,
        timing_artifact,
        timing: request.timing,
        audio_metadata,
        job: state_store.get_job(&job.job_id)?,
        attempt_id: started.attempt.attempt_id,
        cache_hit: false,
        invalidated_step_ids,
        voice_stage_complete,
    })
}

pub fn replace_manual_creator_voice_timing_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    segment_id: &str,
    timing: VoiceTimingV1,
    provenance: ManualResultProvenanceV1,
) -> Result<ManualCreatorVoiceOutcomeV1> {
    let existing = latest_verified_segment_voice_v1(
        state_store,
        artifact_store,
        project_id,
        segment_id,
    )?
    .ok_or_else(|| {
        Error::InvalidArtifact(format!(
            "segment {segment_id} has no selected audio to keep while replacing timing"
        ))
    })?;
    let audio_path = artifact_store.resolve_artifact_path(&existing.audio)?;
    provide_manual_creator_voice_bundle_v1(
        state_store,
        artifact_store,
        ManualCreatorVoiceRequestV1 {
            project_id,
            segment_id,
            audio_path: &audio_path,
            timing,
            provenance,
            replace_existing: true,
        },
    )
}

pub fn creator_segment_voice_states_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
) -> Result<Vec<CreatorSegmentVoiceStateV1>> {
    let (content, _) = load_latest_creator_content_v1(state_store, artifact_store, project_id)?
        .ok_or_else(|| {
            Error::InvalidContract("creator voice status requires verified Content".to_owned())
        })?;
    content
        .segments
        .iter()
        .map(|segment| {
            let selected = latest_verified_segment_voice_v1(
                state_store,
                artifact_store,
                project_id,
                &segment.id,
            )?;
            Ok(match selected {
                Some(value) => CreatorSegmentVoiceStateV1 {
                    segment_id: segment.id.clone(),
                    audio: Some(value.audio),
                    timing_artifact: Some(value.timing_artifact),
                    timing: Some(value.timing),
                    verified: true,
                },
                None => CreatorSegmentVoiceStateV1 {
                    segment_id: segment.id.clone(),
                    audio: None,
                    timing_artifact: None,
                    timing: None,
                    verified: false,
                },
            })
        })
        .collect()
}

pub fn reconcile_creator_voice_aggregate_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
) -> Result<bool> {
    let (content, _) = load_latest_creator_content_v1(state_store, artifact_store, project_id)?
        .ok_or_else(|| {
            Error::InvalidContract("creator voice reconciliation requires verified Content".to_owned())
        })?;
    let parents = creator_voice_parent_steps_v1(state_store, project_id)?;
    let mut all_complete = !content.segments.is_empty();

    for segment in &content.segments {
        let selected = latest_verified_segment_voice_v1(
            state_store,
            artifact_store,
            project_id,
            &segment.id,
        )?;
        let Some(_selected) = selected else {
            all_complete = false;
            continue;
        };
        let step = state_store
            .list_project_steps(project_id)?
            .into_iter()
            .find(|step| step.step == CREATOR_TTS_STEP_V1 && step.unit == segment.id)
            .ok_or_else(|| {
                Error::InvalidJobState(format!(
                    "verified voice segment {} has no canonical tts step",
                    segment.id
                ))
            })?;
        let mut current = state_store.get_step(&step.step_id)?;
        if current.status == StepStatus::Stale {
            state_store.set_step_status(&current.step_id, StepStatus::NotReady)?;
            state_store.refresh_ready_steps(project_id)?;
            current = state_store.get_step(&current.step_id)?;
        }
        if current.status == StepStatus::NotReady {
            state_store.refresh_ready_steps(project_id)?;
            current = state_store.get_step(&current.step_id)?;
        }
        if matches!(current.status, StepStatus::Ready | StepStatus::Retryable) {
            state_store.set_step_status(&current.step_id, StepStatus::Succeeded)?;
        }
        if state_store.get_step(&current.step_id)?.status != StepStatus::Succeeded {
            all_complete = false;
        }
    }

    let mut voice = state_store.get_step(&parents.voice.step_id)?;
    if all_complete {
        if voice.status == StepStatus::Stale {
            state_store.set_step_status(&voice.step_id, StepStatus::NotReady)?;
            state_store.refresh_ready_steps(project_id)?;
            voice = state_store.get_step(&voice.step_id)?;
        }
        if voice.status == StepStatus::NotReady {
            state_store.refresh_ready_steps(project_id)?;
            voice = state_store.get_step(&voice.step_id)?;
        }
        if matches!(voice.status, StepStatus::Ready | StepStatus::Retryable) {
            state_store.set_step_status(&voice.step_id, StepStatus::Succeeded)?;
            state_store.refresh_ready_steps(project_id)?;
        }
        return Ok(state_store.get_step(&voice.step_id)?.status == StepStatus::Succeeded);
    }

    if voice.status != StepStatus::NotReady {
        let impact = state_store.invalidate_from(&voice.step_id, None)?;
        normalize_stale_impact_v1(state_store, &voice.step_id, &impact)?;
    }
    Ok(false)
}

impl ArtifactStore {
    pub fn promote_local_voice_bundle_v1(
        &self,
        state_store: &mut StateStore,
        attempt_id: &str,
        audio_source: &Path,
        audio_uri: LogicalUri,
        timing_source: &Path,
        timing_uri: LogicalUri,
        metadata: serde_json::Value,
    ) -> Result<crate::RemoteVoiceArtifactBundleV1> {
        let attempt = state_store.get_attempt(attempt_id)?;
        let job = state_store.get_job(&attempt.job_id)?;
        if !matches!(attempt.status, StepStatus::Running | StepStatus::Retryable)
            || !matches!(job.status, StepStatus::Running | StepStatus::Retryable)
        {
            return Err(Error::InvalidJobState(
                "local voice bundle promotion requires active voice attempt/job".to_owned(),
            ));
        }
        if !matches!(audio_uri, LogicalUri::Project(_))
            || !matches!(timing_uri, LogicalUri::Project(_))
        {
            return Err(Error::InvalidArtifact(
                "local voice bundle outputs must use project:// logical URIs".to_owned(),
            ));
        }
        if !audio_source.is_file() || !timing_source.is_file() {
            return Err(Error::InvalidArtifact(
                "local voice bundle source files must exist".to_owned(),
            ));
        }
        let timing_contract =
            VoiceTimingV1::from_json_bytes_v1(&fs::read(timing_source)?)?;
        if timing_contract.segment_id != job.unit {
            return Err(Error::InvalidArtifact(
                "local voice timing segment_id does not match logical tts job unit".to_owned(),
            ));
        }

        let resolver = PathResolver::new(self.data_root())?;
        let audio_destination = resolver.resolve(&audio_uri, Some(&job.project_id))?;
        let timing_destination = resolver.resolve(&timing_uri, Some(&job.project_id))?;
        if audio_destination == timing_destination
            || audio_destination.exists()
            || timing_destination.exists()
        {
            return Err(Error::ArtifactTargetExists(if audio_destination.exists() {
                audio_destination
            } else {
                timing_destination
            }));
        }
        let audio_temp = copy_to_voice_temp_v1(audio_source, &audio_destination)?;
        let timing_temp = match copy_to_voice_temp_v1(timing_source, &timing_destination) {
            Ok(path) => path,
            Err(error) => {
                let _ = fs::remove_file(&audio_temp);
                return Err(error);
            }
        };
        let (audio_sha256, audio_size) = sha256_file(&audio_temp)?;
        let (timing_sha256, timing_size) = sha256_file(&timing_temp)?;

        if let Err(error) = fs::rename(&audio_temp, &audio_destination) {
            let _ = fs::remove_file(&audio_temp);
            let _ = fs::remove_file(&timing_temp);
            return Err(error.into());
        }
        if let Err(error) = fs::rename(&timing_temp, &timing_destination) {
            let _ = fs::remove_file(&audio_destination);
            let _ = fs::remove_file(&timing_temp);
            return Err(error.into());
        }

        let mut audio_metadata = metadata.clone();
        merge_json_metadata_v1(
            &mut audio_metadata,
            serde_json::json!({
                "voice_bundle_role": "audio",
                "segment_id": &job.unit,
            }),
        );
        let mut timing_metadata = metadata;
        merge_json_metadata_v1(
            &mut timing_metadata,
            serde_json::json!({
                "voice_bundle_role": "timing",
                "timing_schema": VOICE_TIMING_SCHEMA_V1,
                "segment_id": &job.unit,
                "duration_ms": timing_contract.duration_ms,
                "cue_count": timing_contract.cues.len(),
            }),
        );
        let created_at = Utc::now();
        let audio = Artifact {
            artifact_id: format!("art_{}", Uuid::new_v4().simple()),
            project_id: Some(job.project_id.clone()),
            artifact_type: VOICE_AUDIO_ARTIFACT_TYPE_V1.to_owned(),
            uri: audio_uri,
            sha256: audio_sha256,
            size_bytes: audio_size,
            input_hash: Some(job.input_hash.clone()),
            producer_job: Some(job.job_id.clone()),
            created_at,
            metadata: audio_metadata,
        };
        let timing = Artifact {
            artifact_id: format!("art_{}", Uuid::new_v4().simple()),
            project_id: Some(job.project_id.clone()),
            artifact_type: VOICE_TIMING_ARTIFACT_TYPE_V1.to_owned(),
            uri: timing_uri,
            sha256: timing_sha256,
            size_bytes: timing_size,
            input_hash: Some(job.input_hash.clone()),
            producer_job: Some(job.job_id.clone()),
            created_at,
            metadata: timing_metadata,
        };

        if let Err(error) =
            state_store.commit_voice_bundle_success_v1(attempt_id, &audio, &timing)
        {
            let _ = fs::remove_file(&audio_destination);
            let _ = fs::remove_file(&timing_destination);
            return Err(error);
        }
        if !self.verify_artifact(&audio)? || !self.verify_artifact(&timing)? {
            return Err(Error::InvalidArtifact(
                "local voice bundle did not remain physically verifiable after commit".to_owned(),
            ));
        }
        Ok(crate::RemoteVoiceArtifactBundleV1 { audio, timing })
    }
}

struct CreatorVoiceParentStepsV1 {
    content: WorkflowStep,
    voice: WorkflowStep,
    production: WorkflowStep,
}

fn creator_voice_parent_steps_v1(
    state_store: &StateStore,
    project_id: &str,
) -> Result<CreatorVoiceParentStepsV1> {
    let steps = state_store.list_project_steps(project_id)?;
    let find = |key: &str| {
        steps
            .iter()
            .find(|step| step.step == key && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1)
            .cloned()
            .ok_or_else(|| {
                Error::InvalidJobState(format!(
                    "creator manual voice requires workflow step {key}/project"
                ))
            })
    };
    let content = find(CREATOR_STEP_CONTENT_PREPARE_V1)?;
    let voice = find(CREATOR_STEP_VOICE_PREPARE_V1)?;
    let production = find(CREATOR_STEP_PRODUCTION_PACK_V1)?;
    if content.status != StepStatus::Succeeded {
        return Err(Error::InvalidJobState(
            "content.prepare/project must be SUCCEEDED before manual voice takeover".to_owned(),
        ));
    }
    Ok(CreatorVoiceParentStepsV1 {
        content,
        voice,
        production,
    })
}

fn ensure_manual_tts_step_v1(
    state_store: &StateStore,
    project_id: &str,
    segment_id: &str,
    input_hash: &str,
    replace_existing: bool,
) -> Result<(WorkflowStep, Vec<String>)> {
    let parents = creator_voice_parent_steps_v1(state_store, project_id)?;
    let existing = state_store
        .list_project_steps(project_id)?
        .into_iter()
        .find(|step| step.step == CREATOR_TTS_STEP_V1 && step.unit == segment_id);
    let mut invalidated = Vec::new();
    let step = match existing {
        Some(step) if step.input_hash.as_deref() != Some(input_hash) => {
            if !replace_existing && step.status == StepStatus::Succeeded {
                return Err(Error::InvalidJobState(format!(
                    "tts/{segment_id} already succeeded; replacement must be explicit"
                )));
            }
            let impact = state_store.invalidate_from(&step.step_id, Some(input_hash))?;
            invalidated = impact.iter().map(|item| item.step_id.clone()).collect();
            normalize_stale_impact_v1(state_store, &step.step_id, &impact)?;
            state_store.get_step(&step.step_id)?
        }
        Some(step) => step,
        None => state_store.create_step(
            project_id,
            CREATOR_TTS_STEP_V1,
            segment_id,
            StepStatus::NotReady,
            Some(input_hash),
        )?,
    };
    state_store.add_dependency(&parents.content.step_id, &step.step_id)?;
    state_store.add_dependency(&step.step_id, &parents.voice.step_id)?;
    let _ = &parents.production;
    state_store.refresh_ready_steps(project_id)?;
    Ok((state_store.get_step(&step.step_id)?, invalidated))
}

fn normalize_stale_impact_v1(
    state_store: &StateStore,
    root_step_id: &str,
    impact: &[crate::InvalidationImpact],
) -> Result<()> {
    for affected in impact {
        let current = state_store.get_step(&affected.step_id)?;
        if current.status != StepStatus::Stale {
            continue;
        }
        let next = if affected.step_id == root_step_id {
            StepStatus::NotReady
        } else {
            StepStatus::NotReady
        };
        state_store.set_step_status(&affected.step_id, next)?;
    }
    Ok(())
}

fn latest_verified_segment_voice_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    segment_id: &str,
) -> Result<Option<VerifiedSegmentVoiceV1>> {
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
        let (Some(audio), Some(timing_artifact)) = (take.artifact, take.timing_artifact) else {
            continue;
        };
        if !take.selected
            || audio.project_id.as_deref() != Some(project_id)
            || audio.producer_job.as_deref() != Some(job.job_id.as_str())
            || audio.input_hash.as_deref() != Some(job.input_hash.as_str())
            || timing_artifact.producer_job.as_deref() != Some(job.job_id.as_str())
            || timing_artifact.input_hash.as_deref() != Some(job.input_hash.as_str())
            || !artifact_store.verify_artifact(&audio)?
            || !artifact_store.verify_artifact(&timing_artifact)?
        {
            continue;
        }
        let Some(timing) = artifact_store.load_voice_timing_v1(state_store, attempt_id)? else {
            continue;
        };
        if timing.segment_id != segment_id {
            continue;
        }
        candidates.push((
            audio.created_at,
            audio.artifact_id.clone(),
            VerifiedSegmentVoiceV1 {
                job,
                audio,
                timing_artifact,
                timing,
                attempt_id: attempt_id.to_owned(),
            },
        ));
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    Ok(candidates.pop().map(|(_, _, value)| value))
}

fn verified_voice_for_input_hash_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    segment_id: &str,
    input_hash: &str,
) -> Result<Option<VerifiedSegmentVoiceV1>> {
    Ok(latest_verified_segment_voice_v1(
        state_store,
        artifact_store,
        project_id,
        segment_id,
    )?
    .filter(|value| value.job.input_hash == input_hash))
}

fn parse_srt_v1(
    bytes: &[u8],
    segment_id: &str,
    audio_duration_ms: Option<u64>,
) -> Result<VoiceTimingV1> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::InvalidContract("SRT timing must be UTF-8".to_owned()))?
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let lines = block
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>();
        if lines.is_empty() {
            continue;
        }
        let timing_index = lines
            .iter()
            .position(|line| line.contains("-->"))
            .ok_or_else(|| {
                Error::InvalidContract("SRT cue is missing a '-->' timing line".to_owned())
            })?;
        let timing_line = lines[timing_index];
        let (start, end) = timing_line.split_once("-->").ok_or_else(|| {
            Error::InvalidContract("invalid SRT timing separator".to_owned())
        })?;
        let start_ms = parse_srt_time_v1(start.trim())?;
        let end_ms = parse_srt_time_v1(end.trim())?;
        let cue_text = lines[(timing_index + 1)..].join("\n").trim().to_owned();
        if cue_text.is_empty() {
            return Err(Error::InvalidContract(
                "SRT cue text must not be empty".to_owned(),
            ));
        }
        cues.push(VoiceTimingCueV1 {
            index: u32::try_from(cues.len()).map_err(|_| {
                Error::InvalidContract("SRT cue count exceeds u32 range".to_owned())
            })?,
            text: cue_text,
            start_ms,
            end_ms,
        });
    }
    let max_end = cues.iter().map(|cue| cue.end_ms).max().unwrap_or(0);
    let duration_ms = audio_duration_ms.unwrap_or(max_end);
    let timing = VoiceTimingV1 {
        schema: VOICE_TIMING_SCHEMA_V1.to_owned(),
        version: 1,
        segment_id: segment_id.to_owned(),
        duration_ms,
        cues,
    };
    timing.validate_v1()?;
    Ok(timing)
}

fn parse_srt_time_v1(value: &str) -> Result<u64> {
    let normalized = value.replace('.', ",");
    let (hms, millis) = normalized.split_once(',').ok_or_else(|| {
        Error::InvalidContract(format!("invalid SRT timestamp: {value}"))
    })?;
    let parts = hms.split(':').collect::<Vec<_>>();
    if parts.len() != 3 || millis.len() != 3 {
        return Err(Error::InvalidContract(format!(
            "invalid SRT timestamp: {value}"
        )));
    }
    let hours = parts[0]
        .parse::<u64>()
        .map_err(|_| Error::InvalidContract(format!("invalid SRT hours: {value}")))?;
    let minutes = parts[1]
        .parse::<u64>()
        .map_err(|_| Error::InvalidContract(format!("invalid SRT minutes: {value}")))?;
    let seconds = parts[2]
        .parse::<u64>()
        .map_err(|_| Error::InvalidContract(format!("invalid SRT seconds: {value}")))?;
    let millis = millis
        .parse::<u64>()
        .map_err(|_| Error::InvalidContract(format!("invalid SRT millis: {value}")))?;
    if minutes >= 60 || seconds >= 60 {
        return Err(Error::InvalidContract(format!(
            "invalid SRT clock range: {value}"
        )));
    }
    hours
        .checked_mul(3_600_000)
        .and_then(|value| value.checked_add(minutes * 60_000))
        .and_then(|value| value.checked_add(seconds * 1_000))
        .and_then(|value| value.checked_add(millis))
        .ok_or_else(|| Error::InvalidContract("SRT timestamp overflow".to_owned()))
}

fn format_srt_time_v1(value: u64) -> String {
    let hours = value / 3_600_000;
    let minutes = (value % 3_600_000) / 60_000;
    let seconds = (value % 60_000) / 1_000;
    let millis = value % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

fn validate_timing_against_audio_v1(
    timing: &VoiceTimingV1,
    audio_duration_ms: Option<u64>,
) -> Result<()> {
    timing.validate_v1()?;
    let Some(audio_duration_ms) = audio_duration_ms else {
        return Ok(());
    };
    let difference = timing.duration_ms.abs_diff(audio_duration_ms);
    if difference > AUDIO_DURATION_TOLERANCE_MS_V1 {
        return Err(Error::InvalidContract(format!(
            "voice timing duration {}ms differs from inspected audio duration {}ms by more than {}ms",
            timing.duration_ms, audio_duration_ms, AUDIO_DURATION_TOLERANCE_MS_V1
        )));
    }
    if timing
        .cues
        .iter()
        .any(|cue| cue.end_ms > audio_duration_ms.saturating_add(AUDIO_DURATION_TOLERANCE_MS_V1))
    {
        return Err(Error::InvalidContract(
            "voice timing cue exceeds inspected audio duration".to_owned(),
        ));
    }
    Ok(())
}

fn wav_duration_ms_v1(bytes: &[u8]) -> Result<u64> {
    if bytes.len() < 44 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(Error::InvalidContract(
            "manual WAV does not contain a valid RIFF/WAVE header".to_owned(),
        ));
    }
    let mut offset = 12usize;
    let mut byte_rate = None;
    let mut data_size = None;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = usize::try_from(u32::from_le_bytes(
            bytes[offset + 4..offset + 8].try_into().unwrap(),
        ))
        .map_err(|_| Error::InvalidContract("WAV chunk size overflow".to_owned()))?;
        let start = offset + 8;
        let end = start.saturating_add(size);
        if end > bytes.len() {
            break;
        }
        if id == b"fmt " && size >= 16 {
            let rate = u32::from_le_bytes(bytes[start + 8..start + 12].try_into().unwrap());
            if rate == 0 {
                return Err(Error::InvalidContract(
                    "WAV byte_rate must be positive".to_owned(),
                ));
            }
            byte_rate = Some(u64::from(rate));
        } else if id == b"data" {
            data_size = Some(size as u64);
        }
        offset = end + (size % 2);
    }
    let byte_rate = byte_rate.ok_or_else(|| {
        Error::InvalidContract("WAV is missing a valid fmt chunk".to_owned())
    })?;
    let data_size = data_size.ok_or_else(|| {
        Error::InvalidContract("WAV is missing a data chunk".to_owned())
    })?;
    let duration_ms = data_size
        .checked_mul(1_000)
        .and_then(|value| value.checked_div(byte_rate))
        .unwrap_or(0);
    if duration_ms == 0 {
        return Err(Error::InvalidContract(
            "WAV duration must be positive".to_owned(),
        ));
    }
    Ok(duration_ms)
}

fn validate_mp3_v1(bytes: &[u8]) -> Result<()> {
    if bytes.len() < 4 {
        return Err(Error::InvalidContract(
            "manual MP3 is too small".to_owned(),
        ));
    }
    if bytes.starts_with(b"ID3") {
        return Ok(());
    }
    if bytes[0] == 0xff && (bytes[1] & 0xe0) == 0xe0 {
        return Ok(());
    }
    Err(Error::InvalidContract(
        "manual MP3 does not contain ID3 or MPEG frame sync".to_owned(),
    ))
}

fn copy_to_voice_temp_v1(source: &Path, destination: &Path) -> Result<PathBuf> {
    let parent = destination.parent().ok_or_else(|| {
        Error::InvalidArtifact("manual voice target has no parent".to_owned())
    })?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".manual-voice-{}.tmp", Uuid::new_v4().simple()));
    fs::copy(source, &temp)?;
    let file = fs::OpenOptions::new().read(true).write(true).open(&temp)?;
    file.sync_all()?;
    Ok(temp)
}

fn merge_json_metadata_v1(target: &mut serde_json::Value, extra: serde_json::Value) {
    if !target.is_object() {
        *target = serde_json::json!({});
    }
    if let (Some(target), Some(extra)) = (target.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
}

fn manual_voice_worker_v1(provenance: &ManualResultProvenanceV1) -> &'static str {
    match provenance.producer {
        ManualResultProducerV1::ManualEditor => "manual-result:editor",
        ManualResultProducerV1::LocalFileImport => "manual-result:local-file",
        ManualResultProducerV1::ExternalResultImport => "manual-result:external",
    }
}

fn manual_voice_timing_staging_path_v1(artifact_store: &ArtifactStore) -> Result<PathBuf> {
    Ok(artifact_store
        .data_root()
        .join(".omnicreator")
        .join("manual-staging")
        .join(format!("voice-timing-{}.json", Uuid::new_v4().simple())))
}

fn read_limited_v1(path: &Path, max_bytes: u64, label: &str) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(Error::InvalidContract(format!(
            "{label} import must be a regular file"
        )));
    }
    if metadata.len() > max_bytes {
        return Err(Error::InvalidContract(format!(
            "{label} import exceeds the {max_bytes}-byte safety limit"
        )));
    }
    Ok(fs::read(path)?)
}

fn extension_v1(path: &Path, label: &str) -> Result<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| Error::InvalidContract(format!("{label} has no file extension")))
}
