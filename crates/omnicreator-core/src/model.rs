use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::path_resolver::LogicalUri;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub studio_pack: Option<String>,
    pub channel_profile: Option<String>,
    pub script_version: i64,
    pub production_lock: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StepStatus {
    NotReady,
    Ready,
    Queued,
    Running,
    Succeeded,
    Failed,
    Retryable,
    Fatal,
    Stale,
    Skipped,
    Cancelled,
}

impl StepStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotReady => "NOT_READY",
            Self::Ready => "READY",
            Self::Queued => "QUEUED",
            Self::Running => "RUNNING",
            Self::Succeeded => "SUCCEEDED",
            Self::Failed => "FAILED",
            Self::Retryable => "RETRYABLE",
            Self::Fatal => "FATAL",
            Self::Stale => "STALE",
            Self::Skipped => "SKIPPED",
            Self::Cancelled => "CANCELLED",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectDisplayStatus {
    Draft,
    Preparing,
    NeedsReview,
    GpuReady,
    GpuRunning,
    GpuPartial,
    ReadyForEdit,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Job {
    pub job_id: String,
    pub project_id: String,
    pub step: String,
    pub unit: String,
    pub status: StepStatus,
    pub input_hash: String,
    pub selected_attempt: Option<String>,
    pub selected_artifact: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Attempt {
    pub attempt_id: String,
    pub job_id: String,
    pub worker: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub runtime_seconds: Option<f64>,
    pub status: StepStatus,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Artifact {
    pub artifact_id: String,
    pub project_id: Option<String>,
    pub artifact_type: String,
    pub uri: LogicalUri,
    pub sha256: String,
    pub size_bytes: u64,
    pub input_hash: Option<String>,
    pub producer_job: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowStep {
    pub step_id: String,
    pub project_id: String,
    pub step: String,
    pub unit: String,
    pub status: StepStatus,
    pub input_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvalidationImpact {
    pub step_id: String,
    pub step: String,
    pub unit: String,
    pub previous_status: StepStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FailureDisposition {
    Retryable,
    Fatal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReconciliationSummary {
    pub jobs_marked_retryable: usize,
    pub attempts_marked_retryable: usize,
}
