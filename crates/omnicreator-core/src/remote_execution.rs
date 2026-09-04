use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    fs_util::sha256_file, Artifact, ArtifactStore, ComputeProviderConnectionState, Error,
    FailureDisposition, GpuJobPreparationV1, GpuQueueEligibilityV1, LogicalUri, PathResolver,
    Result, StateStore, StepStatus,
};

pub const REMOTE_EXECUTION_SCHEMA_V1: &str = "omnicreator.compute.remote-execution";
pub const REMOTE_JOURNAL_SCHEMA_V1: &str = "omnicreator.compute.remote-journal";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteComputeJobSpecV1 {
    pub job_id: String,
    pub operation: String,
    #[serde(default)]
    pub plugin_payload: serde_json::Value,
}

impl RemoteComputeJobSpecV1 {
    pub fn validate_v1(&self) -> Result<()> {
        require_identifier("remote job job_id", &self.job_id)?;
        require_identifier("remote job operation", &self.operation)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeJobDispatchV1 {
    pub schema: String,
    pub version: u32,
    pub provider_id: String,
    pub session_id: String,
    pub device_id: String,
    pub job_id: String,
    pub attempt_id: String,
    pub input_hash: String,
    pub plugin_id: String,
    pub operation: String,
    pub model_id: String,
    pub model_version: String,
    pub settings_fingerprint: String,
    pub output_uri: LogicalUri,
    #[serde(default)]
    pub plugin_payload: serde_json::Value,
}

impl ComputeJobDispatchV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != REMOTE_EXECUTION_SCHEMA_V1 || self.version != 1 {
            return Err(Error::InvalidContract(
                "unsupported remote execution schema/version".to_owned(),
            ));
        }
        for (label, value) in [
            ("dispatch provider_id", self.provider_id.as_str()),
            ("dispatch session_id", self.session_id.as_str()),
            ("dispatch device_id", self.device_id.as_str()),
            ("dispatch job_id", self.job_id.as_str()),
            ("dispatch attempt_id", self.attempt_id.as_str()),
            ("dispatch input_hash", self.input_hash.as_str()),
            ("dispatch plugin_id", self.plugin_id.as_str()),
            ("dispatch operation", self.operation.as_str()),
            ("dispatch model_id", self.model_id.as_str()),
            ("dispatch model_version", self.model_version.as_str()),
            (
                "dispatch settings_fingerprint",
                self.settings_fingerprint.as_str(),
            ),
        ] {
            require_identifier(label, value)?;
        }
        if matches!(self.output_uri, LogicalUri::Artifact(_)) {
            return Err(Error::InvalidContract(
                "dispatch output_uri must resolve to a physical logical URI".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeJobDispatchAckV1 {
    pub job_id: String,
    pub attempt_id: String,
    pub remote_job_ref: String,
}

impl ComputeJobDispatchAckV1 {
    pub fn validate_for(&self, dispatch: &ComputeJobDispatchV1) -> Result<()> {
        require_identifier(
            "dispatch acknowledgement remote_job_ref",
            &self.remote_job_ref,
        )?;
        if self.job_id != dispatch.job_id || self.attempt_id != dispatch.attempt_id {
            return Err(Error::InvalidContract(
                "dispatch acknowledgement does not match job/attempt identity".to_owned(),
            ));
        }
        Ok(())
    }
}

pub trait ComputeProviderExecution {
    fn dispatch_job(&mut self, dispatch: &ComputeJobDispatchV1) -> Result<ComputeJobDispatchAckV1>;

    fn read_journal(
        &mut self,
        provider_id: &str,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<Vec<ComputeRemoteJournalEntryV1>>;

    fn transfer_artifact(
        &mut self,
        provider_id: &str,
        session_id: &str,
        transfer_ref: &str,
        destination: &Path,
    ) -> Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeRemoteArtifactV1 {
    pub artifact_type: String,
    pub output_uri: LogicalUri,
    pub sha256: String,
    pub size_bytes: u64,
    pub transfer_ref: String,
}

impl ComputeRemoteArtifactV1 {
    pub fn validate_v1(&self) -> Result<()> {
        require_identifier("remote artifact artifact_type", &self.artifact_type)?;
        require_identifier("remote artifact transfer_ref", &self.transfer_ref)?;
        validate_sha256(&self.sha256)?;
        if matches!(self.output_uri, LogicalUri::Artifact(_)) {
            return Err(Error::InvalidContract(
                "remote artifact output_uri must resolve to a physical logical URI".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ComputeRemoteJournalEventV1 {
    Accepted,
    Running,
    ArtifactReady {
        artifact: ComputeRemoteArtifactV1,
    },
    Failed {
        error_code: String,
        message: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeRemoteJournalEntryV1 {
    pub schema: String,
    pub version: u32,
    pub sequence: u64,
    pub provider_id: String,
    pub session_id: String,
    pub job_id: String,
    pub attempt_id: String,
    pub input_hash: String,
    pub event: ComputeRemoteJournalEventV1,
}

impl ComputeRemoteJournalEntryV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != REMOTE_JOURNAL_SCHEMA_V1 || self.version != 1 {
            return Err(Error::InvalidContract(
                "unsupported remote journal schema/version".to_owned(),
            ));
        }
        if self.sequence == 0 {
            return Err(Error::InvalidContract(
                "remote journal sequence must be positive".to_owned(),
            ));
        }
        for (label, value) in [
            ("journal provider_id", self.provider_id.as_str()),
            ("journal session_id", self.session_id.as_str()),
            ("journal job_id", self.job_id.as_str()),
            ("journal attempt_id", self.attempt_id.as_str()),
            ("journal input_hash", self.input_hash.as_str()),
        ] {
            require_identifier(label, value)?;
        }

        match &self.event {
            ComputeRemoteJournalEventV1::ArtifactReady { artifact } => artifact.validate_v1()?,
            ComputeRemoteJournalEventV1::Failed {
                error_code,
                message,
            } => {
                require_identifier("journal failure error_code", error_code)?;
                if let Some(message) = message {
                    if message.trim().is_empty() {
                        return Err(Error::InvalidContract(
                            "journal failure message must not be blank when present".to_owned(),
                        ));
                    }
                }
            }
            ComputeRemoteJournalEventV1::Accepted | ComputeRemoteJournalEventV1::Running => {}
        }

        Ok(())
    }

    pub fn to_json_line(&self) -> Result<String> {
        self.validate_v1()?;
        let mut line = serde_json::to_string(self)?;
        line.push('\n');
        Ok(line)
    }

    pub fn from_json_line(line: &str) -> Result<Self> {
        let entry: Self = serde_json::from_str(line)?;
        entry.validate_v1()?;
        Ok(entry)
    }

    pub fn artifact_ready(&self) -> Option<&ComputeRemoteArtifactV1> {
        match &self.event {
            ComputeRemoteJournalEventV1::ArtifactReady { artifact } => Some(artifact),
            _ => None,
        }
    }
}

pub fn parse_remote_journal_jsonl(input: &str) -> Result<Vec<ComputeRemoteJournalEntryV1>> {
    let mut entries = Vec::new();
    let mut previous_sequence = None;

    for line in input.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let entry = ComputeRemoteJournalEntryV1::from_json_line(line)?;
        if let Some(previous) = previous_sequence {
            if entry.sequence <= previous {
                return Err(Error::InvalidContract(format!(
                    "remote journal sequence {} is not strictly greater than {}",
                    entry.sequence, previous
                )));
            }
        }
        previous_sequence = Some(entry.sequence);
        entries.push(entry);
    }

    Ok(entries)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteDispatchStartedV1 {
    pub attempt_id: String,
    pub acknowledgement: ComputeJobDispatchAckV1,
    pub dispatch: ComputeJobDispatchV1,
}

pub fn dispatch_remote_job(
    state_store: &mut StateStore,
    executor: &mut impl ComputeProviderExecution,
    eligibility: &GpuQueueEligibilityV1,
    preparation: &GpuJobPreparationV1,
    spec: &RemoteComputeJobSpecV1,
) -> Result<RemoteDispatchStartedV1> {
    spec.validate_v1()?;
    if eligibility.job_id != spec.job_id || preparation.job_id != spec.job_id {
        return Err(Error::InvalidContract(
            "GPU eligibility, preparation and remote job must share the same logical job_id"
                .to_owned(),
        ));
    }
    if !eligibility.is_gpu_ready() {
        return Err(Error::InvalidJobState(format!(
            "job {} cannot be remotely dispatched without GPU_READY eligibility",
            spec.job_id
        )));
    }
    let selection = eligibility.selection.as_ref().ok_or_else(|| {
        Error::InvalidContract("GPU_READY eligibility must include a device selection".to_owned())
    })?;
    preparation.requirements.validate_scheduling_v1()?;

    let job = state_store.get_job(&spec.job_id)?;
    if !matches!(job.status, StepStatus::Ready | StepStatus::Retryable) {
        return Err(Error::InvalidJobState(format!(
            "job {} cannot be remotely dispatched from {}",
            job.job_id,
            job.status.as_str()
        )));
    }

    let plugin_id = required_prepared_value("plugin_id", preparation.plugin_id.as_deref())?;
    let provider_id = required_prepared_value("provider_id", preparation.provider_id.as_deref())?;
    let model_id = required_prepared_value("model_id", preparation.model_id.as_deref())?;
    let model_version =
        required_prepared_value("model_version", preparation.model_version.as_deref())?;
    let settings_fingerprint = required_prepared_value(
        "settings_fingerprint",
        preparation.settings_fingerprint.as_deref(),
    )?;
    let output_uri = preparation.output_uri.as_ref().ok_or_else(|| {
        Error::InvalidContract(
            "GPU preparation output_uri must be known before dispatch".to_owned(),
        )
    })?;
    if provider_id != selection.provider_id {
        return Err(Error::InvalidContract(
            "GPU preparation provider_id does not match selected provider".to_owned(),
        ));
    }

    let worker = format!(
        "{}/{}/{}",
        selection.provider_id, selection.session_id, selection.device_id
    );
    let attempt = state_store.start_attempt(&job.job_id, Some(&worker))?;
    let dispatch = ComputeJobDispatchV1 {
        schema: REMOTE_EXECUTION_SCHEMA_V1.to_owned(),
        version: 1,
        provider_id: selection.provider_id.clone(),
        session_id: selection.session_id.clone(),
        device_id: selection.device_id.clone(),
        job_id: job.job_id.clone(),
        attempt_id: attempt.attempt_id.clone(),
        input_hash: job.input_hash.clone(),
        plugin_id: plugin_id.to_owned(),
        operation: spec.operation.clone(),
        model_id: model_id.to_owned(),
        model_version: model_version.to_owned(),
        settings_fingerprint: settings_fingerprint.to_owned(),
        output_uri: output_uri.clone(),
        plugin_payload: spec.plugin_payload.clone(),
    };
    dispatch.validate_v1()?;

    let acknowledgement = match executor.dispatch_job(&dispatch) {
        Ok(acknowledgement) => acknowledgement,
        Err(error) => {
            state_store.finish_attempt_failure(&attempt.attempt_id, "PROVIDER_UNAVAILABLE")?;
            return Err(error);
        }
    };

    if let Err(error) = acknowledgement.validate_for(&dispatch) {
        state_store.finish_attempt_failure(&attempt.attempt_id, "PROVIDER_UNAVAILABLE")?;
        return Err(error);
    }

    Ok(RemoteDispatchStartedV1 {
        attempt_id: attempt.attempt_id,
        acknowledgement,
        dispatch,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    rename_all = "SCREAMING_SNAKE_CASE",
    tag = "status",
    content = "artifact"
)]
pub enum RemoteArtifactSyncOutcomeV1 {
    Committed(Artifact),
    AlreadyCommitted(Artifact),
}

impl StateStore {
    pub fn commit_remote_artifact_success(
        &mut self,
        attempt_id: &str,
        artifact: &Artifact,
    ) -> Result<()> {
        let attempt = self.get_attempt(attempt_id)?;
        let job_id = artifact
            .producer_job
            .as_deref()
            .ok_or_else(|| Error::InvalidArtifact("producer_job is required".to_owned()))?;
        if attempt.job_id != job_id {
            return Err(Error::InvalidArtifact(
                "remote artifact producer_job does not match attempt job".to_owned(),
            ));
        }
        if !matches!(attempt.status, StepStatus::Running | StepStatus::Retryable) {
            return Err(Error::InvalidJobState(format!(
                "remote attempt {} must be RUNNING or RETRYABLE during artifact reconciliation",
                attempt.attempt_id
            )));
        }

        let job = self.get_job(job_id)?;
        if !matches!(job.status, StepStatus::Running | StepStatus::Retryable) {
            return Err(Error::InvalidJobState(format!(
                "remote job {} must be RUNNING or RETRYABLE during artifact reconciliation",
                job.job_id
            )));
        }

        let artifact_project_id = artifact
            .project_id
            .as_deref()
            .ok_or_else(|| Error::InvalidArtifact("project_id is required".to_owned()))?;
        let artifact_input_hash = artifact
            .input_hash
            .as_deref()
            .ok_or_else(|| Error::InvalidArtifact("input_hash is required".to_owned()))?;
        if job.project_id != artifact_project_id {
            return Err(Error::InvalidArtifact(
                "remote artifact project_id does not match producer job".to_owned(),
            ));
        }
        if job.input_hash != artifact_input_hash {
            return Err(Error::InvalidArtifact(
                "remote artifact input_hash does not match producer job".to_owned(),
            ));
        }

        let size_bytes = i64::try_from(artifact.size_bytes).map_err(|_| {
            Error::InvalidArtifact("remote artifact size exceeds SQLite INTEGER range".to_owned())
        })?;
        let metadata_json = serde_json::to_string(&artifact.metadata)?;
        let finished_at = Utc::now();
        let runtime_millis = finished_at
            .signed_duration_since(attempt.started_at)
            .num_milliseconds()
            .max(0);
        let runtime_seconds = runtime_millis as f64 / 1000.0;

        let transaction = self.connection.transaction()?;
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
             SET status='SUCCEEDED',selected_attempt_id=?1,selected_artifact_id=?2 \
             WHERE id=?3 AND status IN ('RUNNING','RETRYABLE')",
            params![&attempt.attempt_id, &artifact.artifact_id, &job.job_id],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

impl ArtifactStore {
    pub fn committed_remote_artifact(
        &self,
        state_store: &StateStore,
        entry: &ComputeRemoteJournalEntryV1,
    ) -> Result<Option<Artifact>> {
        entry.validate_v1()?;
        let remote_artifact = entry.artifact_ready().ok_or_else(|| {
            Error::InvalidContract(
                "remote artifact reconciliation requires ARTIFACT_READY journal entry".to_owned(),
            )
        })?;
        let job = state_store.get_job(&entry.job_id)?;
        if job.input_hash != entry.input_hash {
            return Err(Error::InvalidContract(
                "journal input_hash does not match canonical logical job".to_owned(),
            ));
        }
        if job.status != StepStatus::Succeeded {
            return Ok(None);
        }

        let artifact_id = job.selected_artifact.as_deref().ok_or_else(|| {
            Error::InvalidArtifact("SUCCEEDED remote job has no selected_artifact_id".to_owned())
        })?;
        let artifact = state_store.get_artifact(artifact_id)?;
        ensure_delivery_matches_artifact(entry, remote_artifact, &artifact)?;
        if !self.verify_artifact(&artifact)? {
            return Err(Error::InvalidArtifact(
                "selected remote artifact is missing from local Data Root".to_owned(),
            ));
        }
        Ok(Some(artifact))
    }

    pub fn promote_remote_artifact(
        &self,
        state_store: &mut StateStore,
        entry: &ComputeRemoteJournalEntryV1,
        transferred_file: impl AsRef<Path>,
        metadata: serde_json::Value,
    ) -> Result<RemoteArtifactSyncOutcomeV1> {
        entry.validate_v1()?;
        let remote_artifact = entry.artifact_ready().ok_or_else(|| {
            Error::InvalidContract(
                "remote artifact promotion requires ARTIFACT_READY journal entry".to_owned(),
            )
        })?;

        if let Some(existing) = self.committed_remote_artifact(state_store, entry)? {
            return Ok(RemoteArtifactSyncOutcomeV1::AlreadyCommitted(existing));
        }

        let job = state_store.get_job(&entry.job_id)?;
        if !matches!(job.status, StepStatus::Running | StepStatus::Retryable) {
            return Err(Error::InvalidJobState(format!(
                "remote artifact can only commit while job {} is RUNNING or RETRYABLE; found {}",
                job.job_id,
                job.status.as_str()
            )));
        }
        if job.input_hash != entry.input_hash {
            return Err(Error::InvalidContract(
                "journal input_hash does not match canonical logical job".to_owned(),
            ));
        }
        let attempt = state_store.get_attempt(&entry.attempt_id)?;
        if attempt.job_id != job.job_id
            || !matches!(attempt.status, StepStatus::Running | StepStatus::Retryable)
        {
            return Err(Error::InvalidJobState(
                "ARTIFACT_READY must reference the active RUNNING/RETRYABLE attempt".to_owned(),
            ));
        }

        let transferred_file = transferred_file.as_ref();
        if !transferred_file.is_file() {
            return Err(Error::InvalidArtifact(format!(
                "transferred remote artifact does not exist: {}",
                transferred_file.display()
            )));
        }
        verify_file_matches_remote(transferred_file, remote_artifact)?;

        let resolver = PathResolver::new(self.data_root())?;
        let project_context = match &remote_artifact.output_uri {
            LogicalUri::Project(_) => Some(job.project_id.as_str()),
            _ => None,
        };
        let destination = resolver.resolve(&remote_artifact.output_uri, project_context)?;
        if destination.exists() {
            return Err(Error::ArtifactTargetExists(destination));
        }
        let parent = destination
            .parent()
            .ok_or_else(|| Error::InvalidArtifact("target has no parent directory".to_owned()))?;
        fs::create_dir_all(parent)?;

        let temp = parent.join(format!(".remote-artifact-{}.tmp", Uuid::new_v4().simple()));
        if let Err(error) = copy_and_sync(transferred_file, &temp) {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        if let Err(error) = verify_file_matches_remote(&temp, remote_artifact) {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        if let Err(error) = fs::rename(&temp, &destination) {
            let _ = fs::remove_file(&temp);
            return Err(error.into());
        }

        let artifact = Artifact {
            artifact_id: format!("art_{}", Uuid::new_v4().simple()),
            project_id: Some(job.project_id),
            artifact_type: remote_artifact.artifact_type.clone(),
            uri: remote_artifact.output_uri.clone(),
            sha256: remote_artifact.sha256.clone(),
            size_bytes: remote_artifact.size_bytes,
            input_hash: Some(entry.input_hash.clone()),
            producer_job: Some(entry.job_id.clone()),
            created_at: Utc::now(),
            metadata,
        };

        if let Err(error) = state_store.commit_remote_artifact_success(&entry.attempt_id, &artifact)
        {
            let _ = fs::remove_file(&destination);
            return Err(error);
        }

        Ok(RemoteArtifactSyncOutcomeV1::Committed(artifact))
    }
}

pub fn sync_remote_artifact(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    executor: &mut impl ComputeProviderExecution,
    entry: &ComputeRemoteJournalEntryV1,
    runtime_staging_dir: impl AsRef<Path>,
    metadata: serde_json::Value,
) -> Result<RemoteArtifactSyncOutcomeV1> {
    entry.validate_v1()?;
    let artifact = entry.artifact_ready().ok_or_else(|| {
        Error::InvalidContract("remote sync requires an ARTIFACT_READY journal entry".to_owned())
    })?;

    if let Some(existing) = artifact_store.committed_remote_artifact(state_store, entry)? {
        return Ok(RemoteArtifactSyncOutcomeV1::AlreadyCommitted(existing));
    }

    let staging_dir = runtime_staging_dir.as_ref();
    fs::create_dir_all(staging_dir)?;
    let staging_path = staging_dir.join(format!(
        ".omnicreator-remote-transfer-{}.tmp",
        Uuid::new_v4().simple()
    ));

    let transfer_result = executor.transfer_artifact(
        &entry.provider_id,
        &entry.session_id,
        &artifact.transfer_ref,
        &staging_path,
    );
    if let Err(error) = transfer_result {
        let _ = fs::remove_file(&staging_path);
        return Err(error);
    }

    let result =
        artifact_store.promote_remote_artifact(state_store, entry, &staging_path, metadata);
    let _ = fs::remove_file(&staging_path);
    result
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RemoteRetryActionV1 {
    AutomaticRetry,
    Requeue,
    RestartWorkerThenRetry,
    RetryWithSupportedFallback,
    RequiresUserAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteRetryPolicyV1 {
    pub disposition: FailureDisposition,
    pub action: RemoteRetryActionV1,
    pub message: String,
}

pub fn remote_retry_policy_v1(error_code: &str) -> RemoteRetryPolicyV1 {
    let disposition = FailureDisposition::from_error_code(error_code);
    let (action, message) = match error_code {
        "NETWORK_TIMEOUT" => (
            RemoteRetryActionV1::AutomaticRetry,
            "Network interruption is retryable; retry the same logical job.".to_owned(),
        ),
        "WORKER_LOST" => (
            RemoteRetryActionV1::Requeue,
            "The remote worker disappeared; requeue unfinished work.".to_owned(),
        ),
        "MODEL_LOAD_ERROR" => (
            RemoteRetryActionV1::RestartWorkerThenRetry,
            "Restart or reload the worker model before retrying.".to_owned(),
        ),
        "CUDA_OOM" => (
            RemoteRetryActionV1::RetryWithSupportedFallback,
            "Retry only with a scheduler-supported lower-memory fallback strategy.".to_owned(),
        ),
        _ if disposition == FailureDisposition::Retryable => (
            RemoteRetryActionV1::Requeue,
            "The failure is retryable; preserve attempt history and requeue the logical job."
                .to_owned(),
        ),
        _ => (
            RemoteRetryActionV1::RequiresUserAction,
            "The failure is fatal until configuration or input changes.".to_owned(),
        ),
    };
    RemoteRetryPolicyV1 {
        disposition,
        action,
        message,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RemoteReconciliationSummaryV1 {
    pub local_attempts_considered: usize,
    pub journal_entries_read: usize,
    pub artifacts_recovered: usize,
    pub artifacts_already_committed: usize,
    pub attempts_restored_running: usize,
    pub attempts_still_running: usize,
    pub attempts_marked_retryable: usize,
    pub attempts_marked_fatal: usize,
    pub attempts_already_retryable: usize,
    pub attempts_deferred: usize,
}

#[derive(Debug, Clone)]
struct ReconcilableRemoteAttemptV1 {
    attempt_id: String,
    job_id: String,
    input_hash: String,
    attempt_status: StepStatus,
    error_code: Option<String>,
}

impl StateStore {
    fn reconcilable_remote_attempts_v1(
        &self,
        provider_id: &str,
        session_id: &str,
    ) -> Result<Vec<ReconcilableRemoteAttemptV1>> {
        let worker_prefix = format!("{provider_id}/{session_id}/");
        let mut statement = self.connection.prepare(
            "SELECT a.id,a.job_id,a.worker,a.status,j.input_hash,a.error_code \
             FROM attempts a \
             JOIN jobs j ON j.id=a.job_id \
             WHERE a.status IN ('RUNNING','RETRYABLE') \
               AND j.status IN ('RUNNING','RETRYABLE') \
               AND a.worker IS NOT NULL \
             ORDER BY a.started_at,a.id",
        )?;
        let rows = statement.query_map([], |row| {
            let raw_status: String = row.get(3)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                crate::state::parse_step_status(&raw_status, 3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })?;

        let mut attempts = Vec::new();
        for row in rows {
            let (attempt_id, job_id, worker, attempt_status, input_hash, error_code) = row?;
            if worker.starts_with(&worker_prefix) && worker.len() > worker_prefix.len() {
                attempts.push(ReconcilableRemoteAttemptV1 {
                    attempt_id,
                    job_id,
                    input_hash,
                    attempt_status,
                    error_code,
                });
            }
        }
        Ok(attempts)
    }

    fn restore_remote_attempt_running_v1(&mut self, attempt_id: &str) -> Result<()> {
        let attempt = self.get_attempt(attempt_id)?;
        if attempt.status == StepStatus::Running {
            return Ok(());
        }
        if attempt.status != StepStatus::Retryable {
            return Err(Error::InvalidJobState(format!(
                "remote attempt {} cannot be restored from {}",
                attempt.attempt_id,
                attempt.status.as_str()
            )));
        }
        if attempt.error_code.as_deref() != Some("LOCAL_RESTART_PENDING_RECONCILIATION") {
            return Err(Error::InvalidJobState(format!(
                "remote attempt {} cannot resume because retry reason is {:?}",
                attempt.attempt_id, attempt.error_code
            )));
        }
        let job = self.get_job(&attempt.job_id)?;
        if job.status != StepStatus::Retryable {
            return Err(Error::InvalidJobState(format!(
                "remote job {} cannot be restored from {}",
                job.job_id,
                job.status.as_str()
            )));
        }

        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE attempts \
             SET status='RUNNING',finished_at=NULL,runtime_seconds=NULL,error_code=NULL \
             WHERE id=?1 AND status='RETRYABLE'",
            [&attempt.attempt_id],
        )?;
        transaction.execute(
            "UPDATE jobs SET status='RUNNING' WHERE id=?1 AND status='RETRYABLE'",
            [&job.job_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn reconcile_remote_failure_v1(
        &mut self,
        attempt_id: &str,
        error_code: &str,
    ) -> Result<StepStatus> {
        require_identifier("remote reconciliation error_code", error_code)?;
        let attempt = self.get_attempt(attempt_id)?;
        if !matches!(attempt.status, StepStatus::Running | StepStatus::Retryable) {
            return Err(Error::InvalidJobState(format!(
                "remote attempt {} cannot reconcile failure from {}",
                attempt.attempt_id,
                attempt.status.as_str()
            )));
        }
        let job = self.get_job(&attempt.job_id)?;
        if !matches!(job.status, StepStatus::Running | StepStatus::Retryable) {
            return Err(Error::InvalidJobState(format!(
                "remote job {} cannot reconcile failure from {}",
                job.job_id,
                job.status.as_str()
            )));
        }

        let policy = remote_retry_policy_v1(error_code);
        let next = match policy.disposition {
            FailureDisposition::Retryable => StepStatus::Retryable,
            FailureDisposition::Fatal => StepStatus::Fatal,
        };
        let finished_at = Utc::now();
        let runtime_millis = finished_at
            .signed_duration_since(attempt.started_at)
            .num_milliseconds()
            .max(0);
        let runtime_seconds = runtime_millis as f64 / 1000.0;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE attempts \
             SET status=?1,finished_at=COALESCE(finished_at,?2), \
                 runtime_seconds=COALESCE(runtime_seconds,?3),error_code=?4 \
             WHERE id=?5 AND status IN ('RUNNING','RETRYABLE')",
            params![
                next.as_str(),
                finished_at.to_rfc3339(),
                runtime_seconds,
                error_code,
                &attempt.attempt_id
            ],
        )?;
        transaction.execute(
            "UPDATE jobs SET status=?1 \
             WHERE id=?2 AND status IN ('RUNNING','RETRYABLE')",
            params![next.as_str(), &job.job_id],
        )?;
        transaction.commit()?;
        Ok(next)
    }
}

pub fn reconcile_remote_session_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    executor: &mut impl ComputeProviderExecution,
    provider_id: &str,
    session_id: &str,
    provider_state: ComputeProviderConnectionState,
    runtime_staging_dir: impl AsRef<Path>,
) -> Result<RemoteReconciliationSummaryV1> {
    require_identifier("reconciliation provider_id", provider_id)?;
    require_identifier("reconciliation session_id", session_id)?;
    let attempts = state_store.reconcilable_remote_attempts_v1(provider_id, session_id)?;
    let mut summary = RemoteReconciliationSummaryV1 {
        local_attempts_considered: attempts.len(),
        ..RemoteReconciliationSummaryV1::default()
    };

    match provider_state {
        ComputeProviderConnectionState::Lost | ComputeProviderConnectionState::Disconnected => {
            for attempt in attempts {
                if attempt.attempt_status == StepStatus::Retryable {
                    summary.attempts_already_retryable += 1;
                    if attempt.error_code.as_deref() != Some("LOCAL_RESTART_PENDING_RECONCILIATION")
                    {
                        continue;
                    }
                }
                let next =
                    state_store.reconcile_remote_failure_v1(&attempt.attempt_id, "WORKER_LOST")?;
                match next {
                    StepStatus::Retryable => {
                        if attempt.attempt_status != StepStatus::Retryable {
                            summary.attempts_marked_retryable += 1;
                        }
                    }
                    StepStatus::Fatal => summary.attempts_marked_fatal += 1,
                    _ => unreachable!("remote failure only resolves to RETRYABLE or FATAL"),
                }
            }
            return Ok(summary);
        }
        ComputeProviderConnectionState::Unhealthy
        | ComputeProviderConnectionState::Stale
        | ComputeProviderConnectionState::Connecting => {
            summary.attempts_deferred = attempts.len();
            return Ok(summary);
        }
        ComputeProviderConnectionState::Ready => {}
    }

    let journal = executor.read_journal(provider_id, session_id, None)?;
    validate_reconciliation_journal_v1(&journal, provider_id, session_id)?;
    summary.journal_entries_read = journal.len();
    let runtime_staging_dir = runtime_staging_dir.as_ref();

    for attempt in attempts {
        let relevant = journal
            .iter()
            .filter(|entry| entry.attempt_id == attempt.attempt_id)
            .collect::<Vec<_>>();

        for entry in &relevant {
            if entry.job_id != attempt.job_id || entry.input_hash != attempt.input_hash {
                return Err(Error::InvalidContract(format!(
                    "remote journal identity mismatch for attempt {}",
                    attempt.attempt_id
                )));
            }
        }

        if let Some(artifact_entry) = relevant
            .iter()
            .rev()
            .find(|entry| entry.artifact_ready().is_some())
            .copied()
        {
            let outcome = sync_remote_artifact(
                state_store,
                artifact_store,
                executor,
                artifact_entry,
                runtime_staging_dir,
                serde_json::json!({
                    "source": "remote-reconciliation",
                    "provider_id": provider_id,
                    "session_id": session_id,
                    "journal_sequence": artifact_entry.sequence
                }),
            )?;
            match outcome {
                RemoteArtifactSyncOutcomeV1::Committed(_) => summary.artifacts_recovered += 1,
                RemoteArtifactSyncOutcomeV1::AlreadyCommitted(_) => {
                    summary.artifacts_already_committed += 1
                }
            }
            continue;
        }

        if let Some((error_code, _message)) = relevant.iter().rev().find_map(|entry| {
            if let ComputeRemoteJournalEventV1::Failed {
                error_code,
                message,
            } = &entry.event
            {
                Some((error_code.as_str(), message.as_deref()))
            } else {
                None
            }
        }) {
            let next = state_store.reconcile_remote_failure_v1(&attempt.attempt_id, error_code)?;
            match next {
                StepStatus::Retryable => summary.attempts_marked_retryable += 1,
                StepStatus::Fatal => summary.attempts_marked_fatal += 1,
                _ => unreachable!("remote failure only resolves to RETRYABLE or FATAL"),
            }
            continue;
        }

        let remote_still_running = relevant.iter().any(|entry| {
            matches!(
                entry.event,
                ComputeRemoteJournalEventV1::Accepted | ComputeRemoteJournalEventV1::Running
            )
        });
        if remote_still_running {
            if attempt.attempt_status == StepStatus::Retryable {
                state_store.restore_remote_attempt_running_v1(&attempt.attempt_id)?;
                summary.attempts_restored_running += 1;
            } else {
                summary.attempts_still_running += 1;
            }
            continue;
        }

        if attempt.attempt_status == StepStatus::Retryable {
            summary.attempts_already_retryable += 1;
            if attempt.error_code.as_deref() != Some("LOCAL_RESTART_PENDING_RECONCILIATION") {
                continue;
            }
        }
        let next = state_store.reconcile_remote_failure_v1(&attempt.attempt_id, "WORKER_LOST")?;
        match next {
            StepStatus::Retryable => {
                if attempt.attempt_status != StepStatus::Retryable {
                    summary.attempts_marked_retryable += 1;
                }
            }
            StepStatus::Fatal => summary.attempts_marked_fatal += 1,
            _ => unreachable!("remote failure only resolves to RETRYABLE or FATAL"),
        }
    }

    Ok(summary)
}

fn validate_reconciliation_journal_v1(
    journal: &[ComputeRemoteJournalEntryV1],
    provider_id: &str,
    session_id: &str,
) -> Result<()> {
    let mut previous_sequence = None;
    for entry in journal {
        entry.validate_v1()?;
        if entry.provider_id != provider_id || entry.session_id != session_id {
            return Err(Error::InvalidContract(
                "remote reconciliation journal contains an entry from another provider/session"
                    .to_owned(),
            ));
        }
        if let Some(previous) = previous_sequence {
            if entry.sequence <= previous {
                return Err(Error::InvalidContract(format!(
                    "remote reconciliation journal sequence {} is not greater than {}",
                    entry.sequence, previous
                )));
            }
        }
        previous_sequence = Some(entry.sequence);
    }
    Ok(())
}

fn required_prepared_value<'a>(label: &str, value: Option<&'a str>) -> Result<&'a str> {
    match value {
        Some(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(Error::InvalidContract(format!(
            "GPU preparation {label} must be known before dispatch"
        ))),
    }
}

fn ensure_delivery_matches_artifact(
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
            "duplicate remote delivery conflicts with committed local artifact".to_owned(),
        ));
    }
    Ok(())
}

fn verify_file_matches_remote(path: &Path, artifact: &ComputeRemoteArtifactV1) -> Result<()> {
    let (sha256, size_bytes) = sha256_file(path)?;
    if sha256 != artifact.sha256 || size_bytes != artifact.size_bytes {
        return Err(Error::ArtifactHashMismatch(format!(
            "remote transfer {}",
            artifact.transfer_ref
        )));
    }
    Ok(())
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

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64 || !value.as_bytes().iter().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::InvalidContract(
            "remote artifact sha256 must be a 64-character hex digest".to_owned(),
        ));
    }
    Ok(())
}

fn require_identifier(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidContract(format!("{label} must not be empty")));
    }
    Ok(())
}

pub fn runtime_transfer_path(
    runtime_staging_dir: impl AsRef<Path>,
    transfer_name: &str,
) -> Result<PathBuf> {
    require_identifier("runtime transfer name", transfer_name)?;
    if transfer_name.contains('/') || transfer_name.contains('\\') {
        return Err(Error::InvalidContract(
            "runtime transfer name must be a single path component".to_owned(),
        ));
    }
    Ok(runtime_staging_dir.as_ref().join(transfer_name))
}
