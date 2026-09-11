use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
    artifact_store::{AttemptOutputPromotion, AttemptPromotionRequest},
    deterministic_input_hash,
    fs_util::sha256_file,
    Artifact, ArtifactStore, Attempt, Error, Job, LogicalUri, Result, StateStore, StepStatus,
    WorkflowStep,
};

pub const MANUAL_RESULT_SCHEMA_V1: &str = "omnicreator.manual-result";
pub const MANUAL_RESULT_VERSION_V1: u32 = 1;
pub const MANUAL_RESULT_IMPORT_ERROR_V1: &str = "MANUAL_RESULT_IMPORT_ERROR";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManualResultProducerV1 {
    ManualEditor,
    LocalFileImport,
    ExternalResultImport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManualResultProvenanceV1 {
    pub schema: String,
    pub version: u32,
    pub producer: ManualResultProducerV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_sha256: Option<String>,
}

impl ManualResultProvenanceV1 {
    pub fn manual_editor() -> Self {
        Self {
            schema: MANUAL_RESULT_SCHEMA_V1.to_owned(),
            version: MANUAL_RESULT_VERSION_V1,
            producer: ManualResultProducerV1::ManualEditor,
            source_label: None,
            request_sha256: None,
        }
    }

    pub fn local_file() -> Self {
        Self {
            schema: MANUAL_RESULT_SCHEMA_V1.to_owned(),
            version: MANUAL_RESULT_VERSION_V1,
            producer: ManualResultProducerV1::LocalFileImport,
            source_label: None,
            request_sha256: None,
        }
    }

    pub fn external(source_label: impl Into<String>, request_sha256: Option<String>) -> Self {
        Self {
            schema: MANUAL_RESULT_SCHEMA_V1.to_owned(),
            version: MANUAL_RESULT_VERSION_V1,
            producer: ManualResultProducerV1::ExternalResultImport,
            source_label: Some(source_label.into()),
            request_sha256,
        }
    }

    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != MANUAL_RESULT_SCHEMA_V1 || self.version != MANUAL_RESULT_VERSION_V1 {
            return Err(Error::InvalidContract(
                "unsupported manual-result provenance schema/version".to_owned(),
            ));
        }
        if let Some(label) = self.source_label.as_deref() {
            validate_symbolic_label_v1("manual-result source_label", label)?;
        }
        if let Some(hash) = self.request_sha256.as_deref() {
            if !is_sha256_hex_v1(hash) {
                return Err(Error::InvalidContract(
                    "manual-result request_sha256 must be lowercase SHA-256 hex".to_owned(),
                ));
            }
        }
        Ok(())
    }

    fn worker_v1(&self) -> &'static str {
        match self.producer {
            ManualResultProducerV1::ManualEditor => "manual-result:editor",
            ManualResultProducerV1::LocalFileImport => "manual-result:local-file",
            ManualResultProducerV1::ExternalResultImport => "manual-result:external",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ManualResultIngestRequestV1 {
    pub schema: String,
    pub version: u32,
    pub project_id: String,
    pub workflow_step: String,
    pub workflow_unit: String,
    pub job_step: String,
    pub job_unit: String,
    pub artifact_type: String,
    pub target_uri: LogicalUri,
    pub provenance: ManualResultProvenanceV1,
    #[serde(default)]
    pub stage_metadata: serde_json::Value,
    #[serde(default)]
    pub replace_existing: bool,
    #[serde(default = "default_true_v1")]
    pub complete_workflow_step: bool,
}

impl ManualResultIngestRequestV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != MANUAL_RESULT_SCHEMA_V1 || self.version != MANUAL_RESULT_VERSION_V1 {
            return Err(Error::InvalidContract(
                "unsupported manual-result ingest schema/version".to_owned(),
            ));
        }
        for (label, value) in [
            ("manual-result project_id", self.project_id.as_str()),
            ("manual-result workflow_step", self.workflow_step.as_str()),
            ("manual-result workflow_unit", self.workflow_unit.as_str()),
            ("manual-result job_step", self.job_step.as_str()),
            ("manual-result job_unit", self.job_unit.as_str()),
            ("manual-result artifact_type", self.artifact_type.as_str()),
        ] {
            validate_identifier_v1(label, value)?;
        }
        if !matches!(self.target_uri, LogicalUri::Project(_)) {
            return Err(Error::InvalidContract(
                "manual-result canonical outputs must use project:// logical URIs".to_owned(),
            ));
        }
        self.provenance.validate_v1()?;
        validate_portable_metadata_v1(&self.stage_metadata)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ManualResultOutcomeV1 {
    pub job: Job,
    pub attempt: Attempt,
    pub artifact: Artifact,
    pub input_hash: String,
    pub source_sha256: String,
    pub source_size_bytes: u64,
    pub cache_hit: bool,
    pub invalidated_step_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ManualResultHistoryEntryV1 {
    pub job: Job,
    pub selected_attempt: Option<Attempt>,
    pub artifact: Option<Artifact>,
    pub provenance: Option<ManualResultProvenanceV1>,
    pub verified: bool,
}

pub fn ingest_manual_result_file_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    request: &ManualResultIngestRequestV1,
    source: impl AsRef<Path>,
) -> Result<ManualResultOutcomeV1> {
    request.validate_v1()?;
    state_store.get_project(&request.project_id)?;
    let source = source.as_ref();
    if !source.is_file() {
        return Err(Error::InvalidArtifact(format!(
            "manual-result source file does not exist: {}",
            source.display()
        )));
    }
    let (source_sha256, source_size_bytes) = sha256_file(source)?;
    let workflow_step = require_workflow_step_v1(state_store, request)?;
    state_store.refresh_ready_steps(&request.project_id)?;
    let workflow_step = state_store.get_step(&workflow_step.step_id)?;
    if workflow_step.status == StepStatus::NotReady {
        return Err(Error::InvalidTransition(format!(
            "manual-result workflow step {}/{} is still waiting on dependencies",
            request.workflow_step, request.workflow_unit
        )));
    }

    let input_hash = manual_result_input_hash_v1(request, &source_sha256, source_size_bytes)?;

    if let Some((job, attempt, artifact)) = find_verified_cache_v1(
        state_store,
        artifact_store,
        request,
        &input_hash,
        request.replace_existing,
    )? {
        if request.complete_workflow_step {
            mark_workflow_step_succeeded_v1(state_store, &workflow_step)?;
        }
        return Ok(ManualResultOutcomeV1 {
            job,
            attempt,
            artifact,
            input_hash,
            source_sha256,
            source_size_bytes,
            cache_hit: true,
            invalidated_step_ids: Vec::new(),
        });
    }

    let existing = find_current_result_v1(
        state_store,
        artifact_store,
        request,
        request.replace_existing,
    )?;
    if existing.is_some() && !request.replace_existing {
        return Err(Error::InvalidJobState(format!(
            "manual-result {}/{} already has a verified canonical result; use replacement explicitly",
            request.job_step, request.job_unit
        )));
    }

    let mut invalidated_step_ids = Vec::new();
    let current_step = state_store.get_step(&workflow_step.step_id)?;
    if current_step.status == StepStatus::Succeeded {
        if !request.replace_existing {
            return Err(Error::InvalidJobState(format!(
                "workflow step {}/{} already succeeded; replacement must be explicit",
                request.workflow_step, request.workflow_unit
            )));
        }
        let impact = state_store.invalidate_from(&current_step.step_id, None)?;
        invalidated_step_ids = impact.iter().map(|item| item.step_id.clone()).collect();
        normalize_invalidation_v1(state_store, &current_step.step_id, &impact)?;
    } else {
        normalize_step_for_execution_v1(state_store, &current_step)?;
    }

    if request.replace_existing {
        state_store.supersede_logical_job_results_v1(
            &request.project_id,
            &request.job_step,
            &request.job_unit,
            None,
        )?;
    }

    let job = get_or_create_job_v1(state_store, request, &input_hash)?;
    if request.complete_workflow_step {
        let step = state_store.get_step(&workflow_step.step_id)?;
        if step.status == StepStatus::Ready {
            state_store.set_step_status(&step.step_id, StepStatus::Running)?;
        }
    }
    let attempt = state_store.start_attempt(&job.job_id, Some(request.provenance.worker_v1()))?;
    let target_uri = if request.replace_existing {
        replacement_target_uri_v1(&request.target_uri, &job.job_id)?
    } else {
        request.target_uri.clone()
    };

    let promotion = artifact_store.promote_attempt_outputs(
        state_store,
        AttemptPromotionRequest {
            attempt_id: attempt.attempt_id.clone(),
            job_id: job.job_id.clone(),
            outputs: vec![AttemptOutputPromotion {
                source: source.to_path_buf(),
                target_uri,
                artifact_type: request.artifact_type.clone(),
                metadata: serde_json::json!({
                    "manual_result": {
                        "schema": MANUAL_RESULT_SCHEMA_V1,
                        "version": MANUAL_RESULT_VERSION_V1,
                        "provenance": &request.provenance,
                        "workflow_step": &request.workflow_step,
                        "workflow_unit": &request.workflow_unit,
                        "job_step": &request.job_step,
                        "job_unit": &request.job_unit,
                        "source_sha256": &source_sha256,
                        "source_size_bytes": source_size_bytes,
                    },
                    "stage": &request.stage_metadata,
                }),
                expected_sha256: Some(source_sha256.clone()),
            }],
            selected_output_index: 0,
        },
    );

    let artifact = match promotion {
        Ok(mut artifacts) => artifacts.pop().ok_or_else(|| {
            Error::InvalidArtifact("manual-result promotion produced no artifact".to_owned())
        })?,
        Err(error) => {
            let _ = state_store
                .finish_attempt_failure(&attempt.attempt_id, MANUAL_RESULT_IMPORT_ERROR_V1);
            if request.complete_workflow_step {
                if let Ok(step) = state_store.get_step(&workflow_step.step_id) {
                    if step.status == StepStatus::Running {
                        let _ = state_store.set_step_status(&step.step_id, StepStatus::Retryable);
                    }
                }
            }
            return Err(error);
        }
    };

    if !artifact_store.verify_artifact(&artifact)? {
        if request.complete_workflow_step {
            let step = state_store.get_step(&workflow_step.step_id)?;
            if step.status == StepStatus::Running {
                state_store.set_step_status(&step.step_id, StepStatus::Retryable)?;
            }
        }
        return Err(Error::InvalidArtifact(format!(
            "manual-result promoted artifact {} is not physically verifiable",
            artifact.artifact_id
        )));
    }

    if request.complete_workflow_step {
        let step = state_store.get_step(&workflow_step.step_id)?;
        if step.status != StepStatus::Succeeded {
            state_store.set_step_status(&step.step_id, StepStatus::Succeeded)?;
        }
        state_store.refresh_ready_steps(&request.project_id)?;
    }

    let job = state_store.get_job(&job.job_id)?;
    let attempt = state_store.get_attempt(&attempt.attempt_id)?;
    Ok(ManualResultOutcomeV1 {
        job,
        attempt,
        artifact,
        input_hash,
        source_sha256,
        source_size_bytes,
        cache_hit: false,
        invalidated_step_ids,
    })
}

pub fn list_manual_result_history_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    job_step: &str,
    job_unit: &str,
) -> Result<Vec<ManualResultHistoryEntryV1>> {
    state_store.get_project(project_id)?;
    validate_identifier_v1("manual-result job_step", job_step)?;
    validate_identifier_v1("manual-result job_unit", job_unit)?;

    let mut entries = Vec::new();
    for job in state_store.list_project_jobs(project_id)? {
        if job.step != job_step || job.unit != job_unit {
            continue;
        }
        let selected_attempt = match job.selected_attempt.as_deref() {
            Some(id) => Some(state_store.get_attempt(id)?),
            None => None,
        };
        let artifact = match job.selected_artifact.as_deref() {
            Some(id) => Some(state_store.get_artifact(id)?),
            None => None,
        };
        let provenance = artifact
            .as_ref()
            .and_then(|value| parse_provenance_v1(&value.metadata).transpose())
            .transpose()?;
        if provenance.is_none()
            && !selected_attempt
                .as_ref()
                .and_then(|value| value.worker.as_deref())
                .is_some_and(|worker| worker.starts_with("manual-result:"))
        {
            continue;
        }
        let verified = match artifact.as_ref() {
            Some(value) => artifact_store.verify_artifact(value)?,
            None => false,
        };
        entries.push(ManualResultHistoryEntryV1 {
            job,
            selected_attempt,
            artifact,
            provenance,
            verified,
        });
    }
    entries.sort_by(|left, right| {
        let left_time = left.artifact.as_ref().map(|a| a.created_at);
        let right_time = right.artifact.as_ref().map(|a| a.created_at);
        right_time
            .cmp(&left_time)
            .then_with(|| right.job.job_id.cmp(&left.job.job_id))
    });
    Ok(entries)
}

fn manual_result_input_hash_v1(
    request: &ManualResultIngestRequestV1,
    source_sha256: &str,
    source_size_bytes: u64,
) -> Result<String> {
    let provenance = serde_json::to_vec(&request.provenance)?;
    let stage_metadata = serde_json::to_vec(&request.stage_metadata)?;
    let size = source_size_bytes.to_string();
    Ok(deterministic_input_hash(&[
        b"manual-result-input-v1",
        request.project_id.as_bytes(),
        request.workflow_step.as_bytes(),
        request.workflow_unit.as_bytes(),
        request.job_step.as_bytes(),
        request.job_unit.as_bytes(),
        request.artifact_type.as_bytes(),
        provenance.as_slice(),
        stage_metadata.as_slice(),
        source_sha256.as_bytes(),
        size.as_bytes(),
    ]))
}

fn require_workflow_step_v1(
    state_store: &StateStore,
    request: &ManualResultIngestRequestV1,
) -> Result<WorkflowStep> {
    state_store
        .list_project_steps(&request.project_id)?
        .into_iter()
        .find(|step| step.step == request.workflow_step && step.unit == request.workflow_unit)
        .ok_or_else(|| {
            Error::InvalidContract(format!(
                "manual-result workflow step {}/{} does not exist",
                request.workflow_step, request.workflow_unit
            ))
        })
}

fn verify_existing_manual_artifact_v1(
    artifact_store: &ArtifactStore,
    artifact: &Artifact,
    allow_invalid_existing: bool,
) -> Result<bool> {
    match artifact_store.verify_artifact(artifact) {
        Ok(verified) => Ok(verified),
        Err(Error::ArtifactHashMismatch(_)) if allow_invalid_existing => Ok(false),
        Err(error) => Err(error),
    }
}

fn find_verified_cache_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    request: &ManualResultIngestRequestV1,
    input_hash: &str,
    allow_invalid_existing: bool,
) -> Result<Option<(Job, Attempt, Artifact)>> {
    for job in state_store.list_project_jobs(&request.project_id)? {
        if job.step != request.job_step
            || job.unit != request.job_unit
            || job.input_hash != input_hash
            || job.status != StepStatus::Succeeded
        {
            continue;
        }
        let (Some(attempt_id), Some(artifact_id)) = (
            job.selected_attempt.as_deref(),
            job.selected_artifact.as_deref(),
        ) else {
            continue;
        };
        let attempt = state_store.get_attempt(attempt_id)?;
        let artifact = state_store.get_artifact(artifact_id)?;
        if verify_existing_manual_artifact_v1(
            artifact_store,
            &artifact,
            allow_invalid_existing,
        )? && parse_provenance_v1(&artifact.metadata)?.is_some()
        {
            return Ok(Some((job, attempt, artifact)));
        }
    }
    Ok(None)
}

fn find_current_result_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    request: &ManualResultIngestRequestV1,
    allow_invalid_existing: bool,
) -> Result<Option<Artifact>> {
    let mut artifacts = Vec::new();
    for job in state_store.list_project_jobs(&request.project_id)? {
        if job.step != request.job_step
            || job.unit != request.job_unit
            || job.status != StepStatus::Succeeded
        {
            continue;
        }
        let Some(artifact_id) = job.selected_artifact.as_deref() else {
            continue;
        };
        let artifact = state_store.get_artifact(artifact_id)?;
        if verify_existing_manual_artifact_v1(
            artifact_store,
            &artifact,
            allow_invalid_existing,
        )? {
            artifacts.push(artifact);
        }
    }
    artifacts.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.artifact_id.cmp(&left.artifact_id))
    });
    Ok(artifacts.into_iter().next())
}

fn replacement_target_uri_v1(target: &LogicalUri, job_id: &str) -> Result<LogicalUri> {
    let raw = target.to_string();
    let slash = raw.rfind('/').ok_or_else(|| {
        Error::InvalidArtifact("manual-result target URI has no portable file name".to_owned())
    })?;
    let prefix = &raw[..=slash];
    let file_name = &raw[(slash + 1)..];
    let replacement = match file_name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => {
            format!("{prefix}{stem}-replacement-{job_id}.{extension}")
        }
        _ => format!("{raw}-replacement-{job_id}"),
    };
    LogicalUri::parse(&replacement)
}

fn get_or_create_job_v1(
    state_store: &mut StateStore,
    request: &ManualResultIngestRequestV1,
    input_hash: &str,
) -> Result<Job> {
    let matching = state_store
        .list_project_jobs(&request.project_id)?
        .into_iter()
        .find(|job| {
            job.step == request.job_step
                && job.unit == request.job_unit
                && job.input_hash == input_hash
        });

    match matching {
        Some(job) if matches!(job.status, StepStatus::Retryable | StepStatus::Failed) => {
            state_store.prepare_job_retry(&job.job_id)
        }
        Some(job) if job.status == StepStatus::Ready => Ok(job),
        Some(job) if matches!(job.status, StepStatus::Running | StepStatus::Queued) => {
            Err(Error::InvalidJobState(format!(
                "manual-result job {} is already active",
                job.job_id
            )))
        }
        Some(job) if job.status == StepStatus::Fatal => Err(Error::InvalidJobState(format!(
            "manual-result job {} is FATAL for unchanged input",
            job.job_id
        ))),
        _ => state_store.create_job(
            &request.project_id,
            &request.job_step,
            &request.job_unit,
            input_hash,
        ),
    }
}

fn normalize_step_for_execution_v1(state_store: &StateStore, step: &WorkflowStep) -> Result<()> {
    match step.status {
        StepStatus::Ready | StepStatus::Succeeded => Ok(()),
        StepStatus::Stale
        | StepStatus::Retryable
        | StepStatus::Failed
        | StepStatus::Cancelled
        | StepStatus::Skipped => {
            state_store.set_step_status(&step.step_id, StepStatus::Ready)?;
            Ok(())
        }
        StepStatus::NotReady => Err(Error::InvalidTransition(format!(
            "manual-result workflow step {}/{} is NOT_READY",
            step.step, step.unit
        ))),
        StepStatus::Running | StepStatus::Queued => Err(Error::InvalidTransition(format!(
            "manual-result workflow step {}/{} is already active",
            step.step, step.unit
        ))),
        StepStatus::Fatal => Err(Error::InvalidTransition(format!(
            "manual-result workflow step {}/{} is FATAL",
            step.step, step.unit
        ))),
    }
}

fn normalize_invalidation_v1(
    state_store: &StateStore,
    root_step_id: &str,
    impact: &[crate::InvalidationImpact],
) -> Result<()> {
    for affected in impact {
        let step = state_store.get_step(&affected.step_id)?;
        if step.status != StepStatus::Stale {
            continue;
        }
        let next = if affected.step_id == root_step_id {
            StepStatus::Ready
        } else {
            StepStatus::NotReady
        };
        state_store.set_step_status(&affected.step_id, next)?;
    }
    Ok(())
}

fn mark_workflow_step_succeeded_v1(state_store: &StateStore, step: &WorkflowStep) -> Result<()> {
    state_store.refresh_ready_steps(&step.project_id)?;
    let current = state_store.get_step(&step.step_id)?;
    match current.status {
        StepStatus::Succeeded => Ok(()),
        StepStatus::Ready | StepStatus::Running | StepStatus::Queued | StepStatus::Retryable => {
            state_store.set_step_status(&current.step_id, StepStatus::Succeeded)?;
            state_store.refresh_ready_steps(&current.project_id)?;
            Ok(())
        }
        status => Err(Error::InvalidTransition(format!(
            "manual-result cannot mark workflow step {}/{} SUCCEEDED from {}",
            current.step,
            current.unit,
            status.as_str()
        ))),
    }
}

fn parse_provenance_v1(metadata: &serde_json::Value) -> Result<Option<ManualResultProvenanceV1>> {
    let Some(value) = metadata
        .get("manual_result")
        .and_then(|manual| manual.get("provenance"))
    else {
        return Ok(None);
    };
    let provenance: ManualResultProvenanceV1 = serde_json::from_value(value.clone())?;
    provenance.validate_v1()?;
    Ok(Some(provenance))
}

fn validate_identifier_v1(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 256 || value.contains('\0') {
        return Err(Error::InvalidContract(format!(
            "{label} must be a non-empty portable identifier"
        )));
    }
    Ok(())
}

fn validate_symbolic_label_v1(label: &str, value: &str) -> Result<()> {
    validate_identifier_v1(label, value)?;
    let trimmed = value.trim();
    if trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed.contains("://")
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains('\n')
        || trimmed.contains('\r')
        || looks_like_windows_absolute_v1(trimmed)
    {
        return Err(Error::InvalidContract(format!(
            "{label} must be symbolic and must not contain a machine path or URL"
        )));
    }
    Ok(())
}

fn validate_portable_metadata_v1(value: &serde_json::Value) -> Result<()> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let normalized = key.to_ascii_lowercase().replace('-', "_");
                if [
                    "secret",
                    "password",
                    "token",
                    "api_key",
                    "authorization",
                    "cookie",
                ]
                .iter()
                .any(|needle| normalized.contains(needle))
                {
                    return Err(Error::InvalidContract(format!(
                        "manual-result stage metadata key {key} may contain secret material"
                    )));
                }
                validate_portable_metadata_v1(child)?;
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                validate_portable_metadata_v1(child)?;
            }
        }
        serde_json::Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.starts_with('/')
                || trimmed.starts_with("\\\\")
                || trimmed.starts_with("file://")
                || looks_like_windows_absolute_v1(trimmed)
            {
                return Err(Error::InvalidContract(
                    "manual-result stage metadata must not persist absolute machine paths"
                        .to_owned(),
                ));
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

fn is_sha256_hex_v1(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn default_true_v1() -> bool {
    true
}
