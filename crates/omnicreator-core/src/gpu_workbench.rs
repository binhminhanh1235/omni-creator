use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{Attempt, Error, Job, Result, StateStore, StepStatus};

pub const GPU_WORKBENCH_QUEUE_SCHEMA_V1: &str = "omnicreator.gpu-workbench-queue";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuWorkbenchJobV1 {
    pub job: Job,
    #[serde(default)]
    pub attempts: Vec<Attempt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuWorkbenchQueueSnapshotV1 {
    pub schema: String,
    pub version: u32,
    pub selected_project_ids: Vec<String>,
    #[serde(default)]
    pub running: Vec<GpuWorkbenchJobV1>,
    #[serde(default)]
    pub completed: Vec<GpuWorkbenchJobV1>,
    #[serde(default)]
    pub remaining: Vec<GpuWorkbenchJobV1>,
    #[serde(default)]
    pub retryable: Vec<GpuWorkbenchJobV1>,
}

impl GpuWorkbenchQueueSnapshotV1 {
    pub fn total_jobs(&self) -> usize {
        self.running.len()
            + self.completed.len()
            + self.remaining.len()
            + self.retryable.len()
    }
}

impl StateStore {
    pub fn gpu_workbench_queue_snapshot_v1(
        &self,
        project_ids: &[String],
    ) -> Result<GpuWorkbenchQueueSnapshotV1> {
        let selected_project_ids = normalize_project_ids_v1(project_ids)?;
        let jobs = self.list_gpu_batch_candidate_jobs_v1(&selected_project_ids)?;

        let mut snapshot = GpuWorkbenchQueueSnapshotV1 {
            schema: GPU_WORKBENCH_QUEUE_SCHEMA_V1.to_owned(),
            version: 1,
            selected_project_ids,
            running: Vec::new(),
            completed: Vec::new(),
            remaining: Vec::new(),
            retryable: Vec::new(),
        };

        for job in jobs {
            let attempts = self.list_attempts(&job.job_id)?;
            let item = GpuWorkbenchJobV1 { job, attempts };
            match item.job.status {
                StepStatus::Running => snapshot.running.push(item),
                StepStatus::Succeeded => snapshot.completed.push(item),
                StepStatus::Retryable => snapshot.retryable.push(item),
                StepStatus::NotReady
                | StepStatus::Ready
                | StepStatus::Queued
                | StepStatus::Failed
                | StepStatus::Fatal
                | StepStatus::Stale
                | StepStatus::Skipped
                | StepStatus::Cancelled => snapshot.remaining.push(item),
            }
        }

        Ok(snapshot)
    }
}

fn normalize_project_ids_v1(project_ids: &[String]) -> Result<Vec<String>> {
    if project_ids.is_empty() {
        return Err(Error::InvalidContract(
            "GPU Workbench requires at least one selected project".to_owned(),
        ));
    }

    let mut normalized = BTreeSet::new();
    for project_id in project_ids {
        if project_id.trim().is_empty() {
            return Err(Error::InvalidContract(
                "GPU Workbench project_id must not be empty".to_owned(),
            ));
        }
        normalized.insert(project_id.clone());
    }
    Ok(normalized.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::{Artifact, LogicalUri, Workspace};

    #[test]
    fn queue_snapshot_is_derived_from_canonical_jobs_and_attempt_history() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let mut store = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = store.create_project("Burst Project").unwrap();

        let remaining = store
            .create_job(&project.id, "tts", "remaining", "hash-remaining")
            .unwrap();

        let running = store
            .create_job(&project.id, "tts", "running", "hash-running")
            .unwrap();
        let running_attempt = store.start_attempt(&running.job_id, Some("worker-a")).unwrap();

        let retryable = store
            .create_job(&project.id, "tts", "retryable", "hash-retryable")
            .unwrap();
        let retry_attempt = store.start_attempt(&retryable.job_id, Some("worker-b")).unwrap();
        store
            .finish_attempt_failure(&retry_attempt.attempt_id, "NETWORK_TIMEOUT")
            .unwrap();

        let completed = store
            .create_job(&project.id, "tts", "completed", "hash-completed")
            .unwrap();
        let completed_attempt = store
            .start_attempt(&completed.job_id, Some("worker-c"))
            .unwrap();
        store
            .finish_attempt_success(&completed_attempt.attempt_id)
            .unwrap();
        store
            .commit_job_success(&Artifact {
                artifact_id: "artifact-completed".to_owned(),
                project_id: Some(project.id.clone()),
                artifact_type: "audio".to_owned(),
                uri: LogicalUri::parse("artifact://artifact-completed").unwrap(),
                sha256: "a".repeat(64),
                size_bytes: 10,
                input_hash: Some(completed.input_hash.clone()),
                producer_job: Some(completed.job_id.clone()),
                created_at: Utc::now(),
                metadata: serde_json::json!({}),
            })
            .unwrap();

        let snapshot = store
            .gpu_workbench_queue_snapshot_v1(&[project.id.clone(), project.id.clone()])
            .unwrap();

        assert_eq!(snapshot.schema, GPU_WORKBENCH_QUEUE_SCHEMA_V1);
        assert_eq!(snapshot.selected_project_ids, vec![project.id]);
        assert_eq!(snapshot.total_jobs(), 4);
        assert_eq!(snapshot.remaining[0].job.job_id, remaining.job_id);
        assert_eq!(snapshot.running[0].job.job_id, running.job_id);
        assert_eq!(
            snapshot.running[0].attempts[0].attempt_id,
            running_attempt.attempt_id
        );
        assert_eq!(snapshot.retryable[0].job.job_id, retryable.job_id);
        assert_eq!(
            snapshot.retryable[0].attempts[0].error_code.as_deref(),
            Some("NETWORK_TIMEOUT")
        );
        assert_eq!(snapshot.completed[0].job.job_id, completed.job_id);
        assert_eq!(snapshot.completed[0].attempts.len(), 1);
    }

    #[test]
    fn queue_snapshot_requires_a_project_selection() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let store = StateStore::open(workspace.sqlite_path()).unwrap();

        assert!(matches!(
            store.gpu_workbench_queue_snapshot_v1(&[]),
            Err(Error::InvalidContract(_))
        ));
    }
}
