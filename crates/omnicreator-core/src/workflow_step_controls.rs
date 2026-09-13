use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::{Error, Result, StateStore, StepStatus, WorkflowStep};

pub const WORKFLOW_STEP_EXECUTION_POLICY_SCHEMA_V1: &str =
    "omnicreator.workflow-step-execution-policy";
pub const WORKFLOW_STEP_EXECUTION_POLICY_VERSION_V1: u32 = 1;
const POLICY_TABLE_V1: &str = "workflow_step_execution_policies";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStepExecutionPolicyV1 {
    pub schema: String,
    pub version: u32,
    pub step_id: String,
    pub project_id: String,
    pub step: String,
    pub unit: String,
    pub automatic_execution_enabled: bool,
}

impl WorkflowStepExecutionPolicyV1 {
    fn from_step_v1(step: WorkflowStep, automatic_execution_enabled: bool) -> Self {
        Self {
            schema: WORKFLOW_STEP_EXECUTION_POLICY_SCHEMA_V1.to_owned(),
            version: WORKFLOW_STEP_EXECUTION_POLICY_VERSION_V1,
            step_id: step.step_id,
            project_id: step.project_id,
            step: step.step,
            unit: step.unit,
            automatic_execution_enabled,
        }
    }
}

impl StateStore {
    /// Additive portable-state migration for Phase 17.
    ///
    /// Older workspaces remain readable because absence of the table means the
    /// historical/default policy: automatic execution is enabled.
    pub fn ensure_workflow_step_execution_policy_schema_v1(&self) -> Result<()> {
        self.connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS workflow_step_execution_policies (
                step_id TEXT PRIMARY KEY REFERENCES steps(id) ON DELETE CASCADE,
                automatic_execution_enabled INTEGER NOT NULL DEFAULT 1
                    CHECK (automatic_execution_enabled IN (0,1)),
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_workflow_step_execution_policies_enabled
                ON workflow_step_execution_policies(automatic_execution_enabled,step_id);
            "#,
        )?;
        Ok(())
    }

    fn workflow_step_execution_policy_table_exists_v1(&self) -> Result<bool> {
        let exists: i64 = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [POLICY_TABLE_V1],
            |row| row.get(0),
        )?;
        Ok(exists != 0)
    }

    pub fn workflow_step_execution_policy_v1(
        &self,
        step_id: &str,
    ) -> Result<WorkflowStepExecutionPolicyV1> {
        let step = self.get_step(step_id)?;
        let enabled = if self.workflow_step_execution_policy_table_exists_v1()? {
            self.connection
                .query_row(
                    "SELECT automatic_execution_enabled FROM workflow_step_execution_policies WHERE step_id=?1",
                    [step_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .map(|value| value != 0)
                .unwrap_or(true)
        } else {
            true
        };
        Ok(WorkflowStepExecutionPolicyV1::from_step_v1(step, enabled))
    }

    pub fn list_project_workflow_step_execution_policies_v1(
        &self,
        project_id: &str,
    ) -> Result<Vec<WorkflowStepExecutionPolicyV1>> {
        self.get_project(project_id)?;
        self.list_project_steps(project_id)?
            .into_iter()
            .map(|step| {
                let step_id = step.step_id.clone();
                let enabled = if self.workflow_step_execution_policy_table_exists_v1()? {
                    self.connection
                        .query_row(
                            "SELECT automatic_execution_enabled FROM workflow_step_execution_policies WHERE step_id=?1",
                            [&step_id],
                            |row| row.get::<_, i64>(0),
                        )
                        .optional()?
                        .map(|value| value != 0)
                        .unwrap_or(true)
                } else {
                    true
                };
                Ok(WorkflowStepExecutionPolicyV1::from_step_v1(step, enabled))
            })
            .collect()
    }

    pub fn workflow_step_automatic_execution_enabled_v1(
        &self,
        project_id: &str,
        step_key: &str,
        unit_key: &str,
    ) -> Result<bool> {
        let step = self
            .list_project_steps(project_id)?
            .into_iter()
            .find(|step| step.step == step_key && step.unit == unit_key)
            .ok_or_else(|| {
                Error::InvalidContract(format!(
                    "workflow step {step_key}/{unit_key} is missing for project {project_id}"
                ))
            })?;
        Ok(self
            .workflow_step_execution_policy_v1(&step.step_id)?
            .automatic_execution_enabled)
    }

    pub fn set_workflow_step_automatic_execution_v1(
        &self,
        step_id: &str,
        enabled: bool,
    ) -> Result<WorkflowStepExecutionPolicyV1> {
        let step = self.get_step(step_id)?;
        if !enabled {
            let active = self
                .list_project_jobs(&step.project_id)?
                .into_iter()
                .any(|job| matches!(job.status, StepStatus::Queued | StepStatus::Running));
            if active {
                return Err(Error::InvalidTransition(
                    "automatic workflow execution cannot be turned OFF while this project has QUEUED or RUNNING work; finish/cancel/reconcile active work first"
                        .to_owned(),
                ));
            }
        }

        self.ensure_workflow_step_execution_policy_schema_v1()?;
        self.connection.execute(
            "INSERT INTO workflow_step_execution_policies(step_id,automatic_execution_enabled,updated_at) \
             VALUES (?1,?2,?3) \
             ON CONFLICT(step_id) DO UPDATE SET automatic_execution_enabled=excluded.automatic_execution_enabled,updated_at=excluded.updated_at",
            params![step_id, i64::from(enabled), Utc::now().to_rfc3339()],
        )?;
        self.workflow_step_execution_policy_v1(step_id)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::{StepStatus, CREATOR_WORKFLOW_UNIT_PROJECT_V1};

    #[test]
    fn policy_defaults_on_and_persists_across_reopen() {
        let temp = tempdir().unwrap();
        let db = temp.path().join("state.sqlite");
        let store = StateStore::open(&db).unwrap();
        let project = store.create_project("toggle persistence").unwrap();
        let step = store
            .create_step(
                &project.id,
                "content.prepare",
                CREATOR_WORKFLOW_UNIT_PROJECT_V1,
                StepStatus::Ready,
                None,
            )
            .unwrap();

        assert!(
            store
                .workflow_step_execution_policy_v1(&step.step_id)
                .unwrap()
                .automatic_execution_enabled
        );
        assert!(
            !store
                .set_workflow_step_automatic_execution_v1(&step.step_id, false)
                .unwrap()
                .automatic_execution_enabled
        );
        drop(store);

        let reopened = StateStore::open(&db).unwrap();
        assert!(
            !reopened
                .workflow_step_execution_policy_v1(&step.step_id)
                .unwrap()
                .automatic_execution_enabled
        );
    }

    #[test]
    fn disabling_rejects_active_project_work() {
        let temp = tempdir().unwrap();
        let mut store = StateStore::open(temp.path().join("state.sqlite")).unwrap();
        let project = store.create_project("active toggle guard").unwrap();
        let step = store
            .create_step(
                &project.id,
                "voice.prepare",
                CREATOR_WORKFLOW_UNIT_PROJECT_V1,
                StepStatus::Ready,
                None,
            )
            .unwrap();
        let job = store
            .create_job(&project.id, "tts.segment", "S001", "hash")
            .unwrap();
        store
            .start_attempt(&job.job_id, Some("test-worker"))
            .unwrap();

        assert!(matches!(
            store.set_workflow_step_automatic_execution_v1(&step.step_id, false),
            Err(Error::InvalidTransition(message)) if message.contains("QUEUED or RUNNING")
        ));
    }
}
