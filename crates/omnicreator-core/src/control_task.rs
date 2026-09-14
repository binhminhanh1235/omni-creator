use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::{Artifact, Attempt, Error, Job, Result, StateStore, StepStatus};

pub const CONTROL_TASK_STEP_PREFIX_V1: &str = "control.agent.";
pub const CONTROL_TASK_CREATOR_START_OR_RESUME_V1: &str = "control.agent.creator-start-or-resume";
pub const CONTROL_TASK_UNIT_PROJECT_V1: &str = "project";
pub const CONTROL_TASK_RESULT_ARTIFACT_TYPE_V1: &str = "control.task-result.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ControlTaskSnapshotV1 {
    pub job: Job,
    pub attempts: Vec<Attempt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_artifact: Option<Artifact>,
}

pub fn is_control_task_step_v1(step: &str) -> bool {
    step.starts_with(CONTROL_TASK_STEP_PREFIX_V1)
}

impl StateStore {
    /// Creates the canonical durable record for an agent/control-plane task.
    ///
    /// The returned Job id is the durable task id. No MCP-specific task table is created.
    /// Control tasks retain a canonical pointer to their active attempt from creation so the
    /// transport can persist or fail the same attempt without inventing process-local state.
    pub fn create_control_task_v1(
        &mut self,
        project_id: &str,
        step: &str,
        input_hash: &str,
        worker: Option<&str>,
    ) -> Result<ControlTaskSnapshotV1> {
        if !is_control_task_step_v1(step) {
            return Err(Error::InvalidJobState(format!(
                "control task step must start with {CONTROL_TASK_STEP_PREFIX_V1}"
            )));
        }
        let job = self.create_job(project_id, step, CONTROL_TASK_UNIT_PROJECT_V1, input_hash)?;
        let attempt = self.start_attempt(&job.job_id, worker)?;
        self.connection.execute(
            "UPDATE jobs SET selected_attempt_id=?1 WHERE id=?2",
            params![&attempt.attempt_id, &job.job_id],
        )?;
        self.get_control_task_v1(&job.job_id)
    }

    pub fn get_control_task_v1(&self, task_id: &str) -> Result<ControlTaskSnapshotV1> {
        let job = self.get_job(task_id)?;
        if !is_control_task_step_v1(&job.step) {
            return Err(Error::InvalidJobState(format!(
                "job {task_id} is not a control task"
            )));
        }
        let attempts = self.list_attempts(task_id)?;
        let selected_artifact = job
            .selected_artifact
            .as_deref()
            .map(|artifact_id| self.get_artifact(artifact_id))
            .transpose()?;
        Ok(ControlTaskSnapshotV1 {
            job,
            attempts,
            selected_artifact,
        })
    }

    /// Cooperatively cancels a durable control task.
    ///
    /// Completed tasks are immutable and cancellation is a no-op, which preserves verified
    /// output. For an active attempt, Job and Attempt transition together in one transaction.
    pub fn cancel_control_task_v1(&mut self, task_id: &str) -> Result<ControlTaskSnapshotV1> {
        let job = self.get_job(task_id)?;
        if !is_control_task_step_v1(&job.step) {
            return Err(Error::InvalidJobState(format!(
                "job {task_id} is not a control task"
            )));
        }

        if matches!(job.status, StepStatus::Succeeded | StepStatus::Cancelled) {
            return self.get_control_task_v1(task_id);
        }

        if matches!(
            job.status,
            StepStatus::Fatal | StepStatus::Stale | StepStatus::Skipped
        ) {
            return Err(Error::InvalidTransition(format!(
                "control task {task_id}: {} cannot be cancelled",
                job.status.as_str()
            )));
        }

        let now = Utc::now();
        let transaction = self.connection.transaction()?;
        let running_attempt: Option<(String, String)> = transaction
            .query_row(
                "SELECT id,started_at FROM attempts WHERE job_id=?1 AND status='RUNNING' ORDER BY started_at DESC,id DESC LIMIT 1",
                [task_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        if let Some((attempt_id, started_at)) = running_attempt {
            let started_at = chrono::DateTime::parse_from_rfc3339(&started_at)
                .map_err(|error| {
                    Error::InvalidJobState(format!(
                        "control task attempt has invalid started_at: {error}"
                    ))
                })?
                .with_timezone(&Utc);
            let runtime_seconds = now
                .signed_duration_since(started_at)
                .num_milliseconds()
                .max(0) as f64
                / 1000.0;
            transaction.execute(
                "UPDATE attempts SET status='CANCELLED',finished_at=?1,runtime_seconds=?2,error_code='CONTROL_TASK_CANCELLED' WHERE id=?3 AND status='RUNNING'",
                params![now.to_rfc3339(), runtime_seconds, attempt_id],
            )?;
        }

        transaction.execute(
            "UPDATE jobs SET status='CANCELLED' WHERE id=?1 AND status NOT IN ('SUCCEEDED','CANCELLED')",
            [task_id],
        )?;
        transaction.commit()?;
        self.get_control_task_v1(task_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{deterministic_input_hash, Workspace};

    #[test]
    fn durable_control_task_survives_store_restart_and_cancels_atomically() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        let workspace = Workspace::create(&root).unwrap();
        let mut store = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = store.create_project("Agent Task").unwrap();
        let hash = deterministic_input_hash(&[b"creator-start", project.id.as_bytes()]);
        let created = store
            .create_control_task_v1(
                &project.id,
                CONTROL_TASK_CREATOR_START_OR_RESUME_V1,
                &hash,
                Some("mcp"),
            )
            .unwrap();
        assert_eq!(created.job.status, StepStatus::Running);
        assert_eq!(created.attempts.len(), 1);
        assert_eq!(
            created.job.selected_attempt.as_deref(),
            Some(created.attempts[0].attempt_id.as_str())
        );
        let task_id = created.job.job_id.clone();
        drop(store);

        let mut reopened = StateStore::open(workspace.sqlite_path()).unwrap();
        let recovered = reopened.get_control_task_v1(&task_id).unwrap();
        assert_eq!(recovered.job.status, StepStatus::Running);
        assert_eq!(recovered.attempts[0].status, StepStatus::Running);
        assert_eq!(
            recovered.job.selected_attempt.as_deref(),
            Some(recovered.attempts[0].attempt_id.as_str())
        );

        let cancelled = reopened.cancel_control_task_v1(&task_id).unwrap();
        assert_eq!(cancelled.job.status, StepStatus::Cancelled);
        assert_eq!(cancelled.attempts[0].status, StepStatus::Cancelled);
        assert_eq!(
            cancelled.attempts[0].error_code.as_deref(),
            Some("CONTROL_TASK_CANCELLED")
        );
    }

    #[test]
    fn ordinary_jobs_cannot_be_addressed_as_control_tasks() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let mut store = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = store.create_project("Ordinary").unwrap();
        let job = store
            .create_job(&project.id, "voice", "S01", "hash")
            .unwrap();
        assert!(store.get_control_task_v1(&job.job_id).is_err());
        assert!(store.cancel_control_task_v1(&job.job_id).is_err());
    }
}
