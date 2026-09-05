use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    fs_util::sha256_file,
    runtime_estimates::record_runtime_observation_transaction_v1,
    voice_takes::{
        attach_voice_take_artifact_transaction_v1, clear_voice_retake_request_transaction_v1,
    },
    Artifact, ArtifactStore, ComputeProviderExecution, ComputeRemoteArtifactV1,
    ComputeRemoteJournalEntryV1, Error, LogicalUri, Result, StateStore, StepStatus,
};

pub const VOICE_TIMING_SCHEMA_V1: &str = "omnicreator.voice-timing";
pub const VOICE_AUDIO_ARTIFACT_TYPE_V1: &str = "audio";
pub const VOICE_TIMING_ARTIFACT_TYPE_V1: &str = "voice_timing";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceTimingCueV1 {
    pub index: u32,
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceTimingV1 {
    pub schema: String,
    pub version: u32,
    pub segment_id: String,
    pub duration_ms: u64,
    #[serde(default)]
    pub cues: Vec<VoiceTimingCueV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceCaptionCueV1 {
    pub index: u32,
    pub text: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
}

impl VoiceTimingV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != VOICE_TIMING_SCHEMA_V1 || self.version != 1 {
            return Err(Error::InvalidContract(
                "unsupported voice timing schema/version".to_owned(),
            ));
        }
        require_identifier("voice timing segment_id", &self.segment_id)?;
        if self.duration_ms == 0 {
            return Err(Error::InvalidContract(
                "voice timing duration_ms must be greater than zero".to_owned(),
            ));
        }
        if self.cues.is_empty() {
            return Err(Error::InvalidContract(
                "voice timing must contain at least one cue".to_owned(),
            ));
        }

        let mut previous_end = 0_u64;
        for (position, cue) in self.cues.iter().enumerate() {
            let expected_index = u32::try_from(position).map_err(|_| {
                Error::InvalidContract("voice timing cue count exceeds u32 range".to_owned())
            })?;
            if cue.index != expected_index {
                return Err(Error::InvalidContract(format!(
                    "voice timing cue index {} must equal deterministic position {}",
                    cue.index, expected_index
                )));
            }
            require_identifier("voice timing cue text", &cue.text)?;
            if cue.end_ms <= cue.start_ms {
                return Err(Error::InvalidContract(format!(
                    "voice timing cue {} must have end_ms greater than start_ms",
                    cue.index
                )));
            }
            if cue.start_ms < previous_end {
                return Err(Error::InvalidContract(format!(
                    "voice timing cue {} overlaps or precedes the previous cue",
                    cue.index
                )));
            }
            if cue.end_ms > self.duration_ms {
                return Err(Error::InvalidContract(format!(
                    "voice timing cue {} exceeds total duration",
                    cue.index
                )));
            }
            previous_end = cue.end_ms;
        }
        Ok(())
    }

    pub fn from_json_bytes_v1(bytes: &[u8]) -> Result<Self> {
        let timing: Self = serde_json::from_slice(bytes)?;
        timing.validate_v1()?;
        Ok(timing)
    }

    pub fn to_json_bytes_v1(&self) -> Result<Vec<u8>> {
        self.validate_v1()?;
        serde_json::to_vec_pretty(self).map_err(Into::into)
    }

    pub fn caption_cues_v1(&self) -> Result<Vec<VoiceCaptionCueV1>> {
        self.validate_v1()?;
        Ok(self
            .cues
            .iter()
            .map(|cue| VoiceCaptionCueV1 {
                index: cue.index,
                text: cue.text.clone(),
                start_seconds: cue.start_ms as f64 / 1000.0,
                end_seconds: cue.end_ms as f64 / 1000.0,
            })
            .collect())
    }

    pub fn duration_seconds_v1(&self) -> Result<f64> {
        self.validate_v1()?;
        Ok(self.duration_ms as f64 / 1000.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteVoiceArtifactBundleV1 {
    pub audio: Artifact,
    pub timing: Artifact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    rename_all = "SCREAMING_SNAKE_CASE",
    tag = "status",
    content = "bundle"
)]
pub enum RemoteVoiceBundleSyncOutcomeV1 {
    Committed(RemoteVoiceArtifactBundleV1),
    AlreadyCommitted(RemoteVoiceArtifactBundleV1),
}

impl StateStore {
    pub fn selected_voice_timing_artifact_v1(&self, job_id: &str) -> Result<Option<Artifact>> {
        let job = self.get_job(job_id)?;
        let Some(attempt_id) = job.selected_attempt.as_deref() else {
            return Ok(None);
        };
        let Some(take) = self.get_voice_take_v1(attempt_id)? else {
            return Ok(None);
        };
        Ok(take.timing_artifact)
    }

    pub fn commit_remote_voice_bundle_success_v1(
        &mut self,
        attempt_id: &str,
        audio: &Artifact,
        timing: &Artifact,
    ) -> Result<()> {
        let attempt = self.get_attempt(attempt_id)?;
        if !matches!(attempt.status, StepStatus::Running | StepStatus::Retryable) {
            return Err(Error::InvalidJobState(format!(
                "voice bundle attempt {} must be RUNNING or RETRYABLE",
                attempt.attempt_id
            )));
        }
        let job = self.get_job(&attempt.job_id)?;
        if !matches!(job.status, StepStatus::Running | StepStatus::Retryable) {
            return Err(Error::InvalidJobState(format!(
                "voice bundle job {} must be RUNNING or RETRYABLE",
                job.job_id
            )));
        }

        validate_bundle_artifact_v1(&job, audio, VOICE_AUDIO_ARTIFACT_TYPE_V1)?;
        validate_bundle_artifact_v1(&job, timing, VOICE_TIMING_ARTIFACT_TYPE_V1)?;

        let audio_size = i64::try_from(audio.size_bytes).map_err(|_| {
            Error::InvalidArtifact("voice audio size exceeds SQLite INTEGER range".to_owned())
        })?;
        let timing_size = i64::try_from(timing.size_bytes).map_err(|_| {
            Error::InvalidArtifact("voice timing size exceeds SQLite INTEGER range".to_owned())
        })?;
        let audio_metadata = serde_json::to_string(&audio.metadata)?;
        let timing_metadata = serde_json::to_string(&timing.metadata)?;
        let finished_at = Utc::now();
        let runtime_micros = finished_at
            .signed_duration_since(attempt.started_at)
            .num_microseconds()
            .unwrap_or(i64::MAX)
            .max(1);
        let runtime_seconds = runtime_micros as f64 / 1_000_000.0;

        let transaction = self.connection.transaction()?;
        insert_artifact_transaction_v1(&transaction, audio, audio_size, &audio_metadata)?;
        insert_artifact_transaction_v1(&transaction, timing, timing_size, &timing_metadata)?;

        if !attach_voice_take_artifact_transaction_v1(
            &transaction,
            &attempt.attempt_id,
            &audio.artifact_id,
        )? {
            return Err(Error::InvalidArtifact(
                "voice bundle attempt is not registered as a voice take".to_owned(),
            ));
        }
        transaction.execute(
            "INSERT INTO voice_take_timing_artifacts(attempt_id,artifact_id,created_at) \
             VALUES (?1,?2,?3)",
            params![
                &attempt.attempt_id,
                &timing.artifact_id,
                timing.created_at.to_rfc3339(),
            ],
        )?;
        transaction.execute(
            "UPDATE attempts \
             SET status='SUCCEEDED',finished_at=?1,runtime_seconds=?2,error_code=NULL \
             WHERE id=?3 AND status IN ('RUNNING','RETRYABLE')",
            params![
                finished_at.to_rfc3339(),
                runtime_seconds,
                &attempt.attempt_id
            ],
        )?;
        transaction.execute(
            "UPDATE jobs \
             SET status='SUCCEEDED', \
                 selected_attempt_id=COALESCE(selected_attempt_id,?1), \
                 selected_artifact_id=COALESCE(selected_artifact_id,?2) \
             WHERE id=?3 AND status IN ('RUNNING','RETRYABLE')",
            params![&attempt.attempt_id, &audio.artifact_id, &job.job_id],
        )?;
        clear_voice_retake_request_transaction_v1(&transaction, &job.job_id)?;
        record_runtime_observation_transaction_v1(
            &transaction,
            &attempt.attempt_id,
            runtime_seconds,
            finished_at,
        )?;
        transaction.commit()?;
        Ok(())
    }
}

impl ArtifactStore {
    pub fn load_voice_timing_v1(
        &self,
        state_store: &StateStore,
        attempt_id: &str,
    ) -> Result<Option<VoiceTimingV1>> {
        let Some(take) = state_store.get_voice_take_v1(attempt_id)? else {
            return Ok(None);
        };
        let Some(timing_artifact) = take.timing_artifact else {
            return Ok(None);
        };
        if !self.verify_artifact(&timing_artifact)? {
            return Ok(None);
        }
        let path = self.resolve_artifact_path(&timing_artifact)?;
        let bytes = fs::read(path)?;
        let timing = VoiceTimingV1::from_json_bytes_v1(&bytes)?;
        if timing.segment_id != state_store.get_job(&take.attempt.job_id)?.unit {
            return Err(Error::InvalidArtifact(
                "voice timing segment_id does not match logical job unit".to_owned(),
            ));
        }
        Ok(Some(timing))
    }

    pub fn committed_remote_voice_bundle_v1(
        &self,
        state_store: &StateStore,
        entry: &ComputeRemoteJournalEntryV1,
    ) -> Result<Option<RemoteVoiceArtifactBundleV1>> {
        entry.validate_v1()?;
        let remote_artifacts = entry.artifact_bundle_ready().ok_or_else(|| {
            Error::InvalidContract(
                "voice bundle reconciliation requires ARTIFACT_BUNDLE_READY".to_owned(),
            )
        })?;
        let (remote_audio, remote_timing) = voice_bundle_members_v1(remote_artifacts)?;
        let Some(take) = state_store.get_voice_take_v1(&entry.attempt_id)? else {
            return Ok(None);
        };

        match (&take.artifact, &take.timing_artifact) {
            (None, None) => Ok(None),
            (Some(audio), Some(timing)) => {
                ensure_remote_matches_local_v1(entry, remote_audio, audio)?;
                ensure_remote_matches_local_v1(entry, remote_timing, timing)?;
                if !self.verify_artifact(audio)? || !self.verify_artifact(timing)? {
                    return Err(Error::InvalidArtifact(
                        "committed voice bundle is missing from local Data Root".to_owned(),
                    ));
                }
                Ok(Some(RemoteVoiceArtifactBundleV1 {
                    audio: audio.clone(),
                    timing: timing.clone(),
                }))
            }
            _ => Err(Error::InvalidArtifact(
                "voice take has a partially committed audio/timing bundle".to_owned(),
            )),
        }
    }
}

pub fn sync_remote_voice_artifact_bundle_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    executor: &mut impl ComputeProviderExecution,
    entry: &ComputeRemoteJournalEntryV1,
    runtime_staging_dir: impl AsRef<Path>,
    metadata: serde_json::Value,
) -> Result<RemoteVoiceBundleSyncOutcomeV1> {
    entry.validate_v1()?;
    let remote_artifacts = entry.artifact_bundle_ready().ok_or_else(|| {
        Error::InvalidContract("voice sync requires ARTIFACT_BUNDLE_READY".to_owned())
    })?;
    let (remote_audio, remote_timing) = voice_bundle_members_v1(remote_artifacts)?;

    if let Some(existing) = artifact_store.committed_remote_voice_bundle_v1(state_store, entry)? {
        return Ok(RemoteVoiceBundleSyncOutcomeV1::AlreadyCommitted(existing));
    }

    let job = state_store.get_job(&entry.job_id)?;
    if !matches!(job.status, StepStatus::Running | StepStatus::Retryable) {
        return Err(Error::InvalidJobState(format!(
            "voice bundle job {} must be RUNNING or RETRYABLE",
            job.job_id
        )));
    }
    if job.input_hash != entry.input_hash {
        return Err(Error::InvalidContract(
            "voice bundle journal input_hash does not match logical job".to_owned(),
        ));
    }
    let attempt = state_store.get_attempt(&entry.attempt_id)?;
    if attempt.job_id != job.job_id
        || !matches!(attempt.status, StepStatus::Running | StepStatus::Retryable)
    {
        return Err(Error::InvalidJobState(
            "voice bundle must reference the active take attempt".to_owned(),
        ));
    }

    let staging_dir = runtime_staging_dir.as_ref();
    fs::create_dir_all(staging_dir)?;
    let audio_staging = staging_dir.join(format!(
        ".voice-audio-transfer-{}.tmp",
        Uuid::new_v4().simple()
    ));
    let timing_staging = staging_dir.join(format!(
        ".voice-timing-transfer-{}.tmp",
        Uuid::new_v4().simple()
    ));

    let transfer_result = (|| -> Result<()> {
        executor.transfer_artifact(
            &entry.provider_id,
            &entry.session_id,
            &remote_audio.transfer_ref,
            &audio_staging,
        )?;
        verify_file_matches_remote_v1(&audio_staging, remote_audio)?;
        executor.transfer_artifact(
            &entry.provider_id,
            &entry.session_id,
            &remote_timing.transfer_ref,
            &timing_staging,
        )?;
        verify_file_matches_remote_v1(&timing_staging, remote_timing)?;
        Ok(())
    })();
    if let Err(error) = transfer_result {
        let _ = fs::remove_file(&audio_staging);
        let _ = fs::remove_file(&timing_staging);
        return Err(error);
    }

    let timing_contract = match fs::read(&timing_staging)
        .map_err(Error::from)
        .and_then(|bytes| VoiceTimingV1::from_json_bytes_v1(&bytes))
    {
        Ok(timing) => timing,
        Err(error) => {
            let _ = fs::remove_file(&audio_staging);
            let _ = fs::remove_file(&timing_staging);
            return Err(error);
        }
    };
    if timing_contract.segment_id != job.unit {
        let _ = fs::remove_file(&audio_staging);
        let _ = fs::remove_file(&timing_staging);
        return Err(Error::InvalidArtifact(
            "voice timing segment_id does not match logical job unit".to_owned(),
        ));
    }

    let resolver = crate::PathResolver::new(artifact_store.data_root())?;
    let audio_destination = resolve_remote_destination_v1(&resolver, &job, remote_audio)?;
    let timing_destination = resolve_remote_destination_v1(&resolver, &job, remote_timing)?;
    if audio_destination == timing_destination {
        return Err(Error::InvalidArtifact(
            "voice audio and timing artifacts resolve to the same local path".to_owned(),
        ));
    }
    if audio_destination.exists() {
        return Err(Error::ArtifactTargetExists(audio_destination));
    }
    if timing_destination.exists() {
        return Err(Error::ArtifactTargetExists(timing_destination));
    }

    let audio_temp = prepare_destination_temp_v1(&audio_staging, &audio_destination)?;
    let timing_temp = match prepare_destination_temp_v1(&timing_staging, &timing_destination) {
        Ok(path) => path,
        Err(error) => {
            let _ = fs::remove_file(&audio_temp);
            return Err(error);
        }
    };

    if let Err(error) = verify_file_matches_remote_v1(&audio_temp, remote_audio)
        .and_then(|_| verify_file_matches_remote_v1(&timing_temp, remote_timing))
    {
        let _ = fs::remove_file(&audio_temp);
        let _ = fs::remove_file(&timing_temp);
        return Err(error);
    }

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
    merge_metadata_v1(
        &mut audio_metadata,
        serde_json::json!({
            "voice_bundle_role":"audio"
        }),
    );
    let mut timing_metadata = metadata;
    merge_metadata_v1(
        &mut timing_metadata,
        serde_json::json!({
            "voice_bundle_role":"timing",
            "timing_schema":VOICE_TIMING_SCHEMA_V1,
            "segment_id":timing_contract.segment_id,
            "duration_ms":timing_contract.duration_ms,
            "cue_count":timing_contract.cues.len()
        }),
    );

    let audio = remote_to_local_artifact_v1(&job, entry, remote_audio, audio_metadata);
    let timing = remote_to_local_artifact_v1(&job, entry, remote_timing, timing_metadata);

    if let Err(error) =
        state_store.commit_remote_voice_bundle_success_v1(&entry.attempt_id, &audio, &timing)
    {
        let _ = fs::remove_file(&audio_destination);
        let _ = fs::remove_file(&timing_destination);
        return Err(error);
    }

    let _ = fs::remove_file(&audio_staging);
    let _ = fs::remove_file(&timing_staging);
    Ok(RemoteVoiceBundleSyncOutcomeV1::Committed(
        RemoteVoiceArtifactBundleV1 { audio, timing },
    ))
}

pub fn voice_timing_output_uri_v1(audio_uri: &LogicalUri) -> Result<LogicalUri> {
    let (scheme, path) = match audio_uri {
        LogicalUri::Workspace(path) => ("workspace", path.as_str()),
        LogicalUri::Project(path) => ("project", path.as_str()),
        LogicalUri::Library(path) => ("library", path.as_str()),
        LogicalUri::Artifact(_) => {
            return Err(Error::InvalidContract(
                "voice timing URI cannot derive from artifact://".to_owned(),
            ))
        }
    };
    let path = Path::new(path);
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| Error::InvalidContract("voice audio URI has no valid stem".to_owned()))?;
    let mut relative = PathBuf::from(parent);
    relative.push(format!("{stem}.timing.json"));
    let relative = relative
        .to_str()
        .ok_or_else(|| Error::InvalidContract("voice timing URI is not UTF-8".to_owned()))?
        .replace('\\', "/");
    LogicalUri::parse(&format!("{scheme}://{relative}"))
}

fn voice_bundle_members_v1(
    artifacts: &[ComputeRemoteArtifactV1],
) -> Result<(&ComputeRemoteArtifactV1, &ComputeRemoteArtifactV1)> {
    let audio = artifacts
        .iter()
        .filter(|artifact| artifact.artifact_type == VOICE_AUDIO_ARTIFACT_TYPE_V1)
        .collect::<Vec<_>>();
    let timing = artifacts
        .iter()
        .filter(|artifact| artifact.artifact_type == VOICE_TIMING_ARTIFACT_TYPE_V1)
        .collect::<Vec<_>>();
    if artifacts.len() != 2 || audio.len() != 1 || timing.len() != 1 {
        return Err(Error::InvalidContract(
            "voice artifact bundle requires exactly one audio and one voice_timing artifact"
                .to_owned(),
        ));
    }
    let expected_timing_uri = voice_timing_output_uri_v1(&audio[0].output_uri)?;
    if timing[0].output_uri != expected_timing_uri {
        return Err(Error::InvalidContract(
            "voice timing output_uri must be the sidecar of the audio output_uri".to_owned(),
        ));
    }
    Ok((audio[0], timing[0]))
}

fn validate_bundle_artifact_v1(
    job: &crate::Job,
    artifact: &Artifact,
    expected_type: &str,
) -> Result<()> {
    if artifact.producer_job.as_deref() != Some(job.job_id.as_str())
        || artifact.project_id.as_deref() != Some(job.project_id.as_str())
        || artifact.input_hash.as_deref() != Some(job.input_hash.as_str())
    {
        return Err(Error::InvalidArtifact(
            "voice bundle artifact identity does not match logical job".to_owned(),
        ));
    }
    if artifact.artifact_type != expected_type {
        return Err(Error::InvalidArtifact(format!(
            "voice bundle expected artifact type {expected_type}"
        )));
    }
    Ok(())
}

fn insert_artifact_transaction_v1(
    transaction: &rusqlite::Transaction<'_>,
    artifact: &Artifact,
    size_bytes: i64,
    metadata_json: &str,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO artifacts(id,project_id,artifact_type,uri,sha256,size_bytes,producer_job_id,created_at,metadata_json,input_hash) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            &artifact.artifact_id,
            &artifact.project_id,
            &artifact.artifact_type,
            artifact.uri.as_str(),
            &artifact.sha256,
            size_bytes,
            &artifact.producer_job,
            artifact.created_at.to_rfc3339(),
            metadata_json,
            &artifact.input_hash,
        ],
    )?;
    Ok(())
}

fn ensure_remote_matches_local_v1(
    entry: &ComputeRemoteJournalEntryV1,
    remote: &ComputeRemoteArtifactV1,
    local: &Artifact,
) -> Result<()> {
    if local.producer_job.as_deref() != Some(entry.job_id.as_str())
        || local.input_hash.as_deref() != Some(entry.input_hash.as_str())
        || local.artifact_type != remote.artifact_type
        || local.uri != remote.output_uri
        || local.sha256 != remote.sha256
        || local.size_bytes != remote.size_bytes
    {
        return Err(Error::InvalidArtifact(
            "duplicate remote voice bundle conflicts with committed local artifact".to_owned(),
        ));
    }
    Ok(())
}

fn verify_file_matches_remote_v1(path: &Path, artifact: &ComputeRemoteArtifactV1) -> Result<()> {
    let (sha256, size_bytes) = sha256_file(path)?;
    if sha256 != artifact.sha256 || size_bytes != artifact.size_bytes {
        return Err(Error::ArtifactHashMismatch(format!(
            "remote transfer {}",
            artifact.transfer_ref
        )));
    }
    Ok(())
}

fn resolve_remote_destination_v1(
    resolver: &crate::PathResolver,
    job: &crate::Job,
    artifact: &ComputeRemoteArtifactV1,
) -> Result<PathBuf> {
    let project_context = match artifact.output_uri {
        LogicalUri::Project(_) => Some(job.project_id.as_str()),
        _ => None,
    };
    resolver.resolve(&artifact.output_uri, project_context)
}

fn prepare_destination_temp_v1(source: &Path, destination: &Path) -> Result<PathBuf> {
    let parent = destination
        .parent()
        .ok_or_else(|| Error::InvalidArtifact("voice bundle target has no parent".to_owned()))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".voice-bundle-{}.tmp", Uuid::new_v4().simple()));
    fs::copy(source, &temp)?;
    let file = fs::OpenOptions::new().read(true).write(true).open(&temp)?;
    file.sync_all()?;
    Ok(temp)
}

fn remote_to_local_artifact_v1(
    job: &crate::Job,
    entry: &ComputeRemoteJournalEntryV1,
    remote: &ComputeRemoteArtifactV1,
    metadata: serde_json::Value,
) -> Artifact {
    Artifact {
        artifact_id: format!("art_{}", Uuid::new_v4().simple()),
        project_id: Some(job.project_id.clone()),
        artifact_type: remote.artifact_type.clone(),
        uri: remote.output_uri.clone(),
        sha256: remote.sha256.clone(),
        size_bytes: remote.size_bytes,
        input_hash: Some(entry.input_hash.clone()),
        producer_job: Some(job.job_id.clone()),
        created_at: Utc::now(),
        metadata,
    }
}

fn merge_metadata_v1(target: &mut serde_json::Value, extra: serde_json::Value) {
    if !target.is_object() {
        *target = serde_json::json!({});
    }
    let target = target
        .as_object_mut()
        .expect("metadata normalized to object above");
    if let Some(extra) = extra.as_object() {
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
}

fn require_identifier(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidContract(format!("{label} must not be empty")));
    }
    Ok(())
}
