use chrono::{DateTime, Utc};
use rusqlite::{params, types::Type, OptionalExtension};
use uuid::Uuid;

use crate::{
    state::parse_step_status, Artifact, Attempt, Error, FailureDisposition, InvalidationImpact,
    ProjectDisplayStatus, ReconciliationSummary, Result, StateStore, StepStatus, WorkflowStep,
};

impl StepStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        if self == next {
            return true;
        }

        match self {
            Self::NotReady => matches!(
                next,
                Self::Ready | Self::Stale | Self::Skipped | Self::Cancelled
            ),
            Self::Ready => matches!(
                next,
                Self::Queued
                    | Self::Running
                    | Self::Succeeded
                    | Self::Stale
                    | Self::Skipped
                    | Self::Cancelled
            ),
            Self::Queued => matches!(
                next,
                Self::Running
                    | Self::Succeeded
                    | Self::Failed
                    | Self::Retryable
                    | Self::Fatal
                    | Self::Stale
                    | Self::Cancelled
            ),
            Self::Running => matches!(
                next,
                Self::Succeeded
                    | Self::Failed
                    | Self::Retryable
                    | Self::Fatal
                    | Self::Stale
                    | Self::Cancelled
            ),
            Self::Succeeded => matches!(next, Self::Stale),
            Self::Failed => matches!(
                next,
                Self::Ready | Self::Retryable | Self::Fatal | Self::Stale | Self::Cancelled
            ),
            Self::Retryable => matches!(
                next,
                Self::Ready
                    | Self::Queued
                    | Self::Running
                    | Self::Succeeded
                    | Self::Fatal
                    | Self::Stale
                    | Self::Cancelled
            ),
            Self::Fatal => matches!(next, Self::Stale),
            Self::Stale => matches!(
                next,
                Self::NotReady | Self::Ready | Self::Skipped | Self::Cancelled
            ),
            Self::Skipped => matches!(next, Self::Ready | Self::Stale),
            Self::Cancelled => matches!(next, Self::Ready | Self::Stale),
        }
    }
}

impl FailureDisposition {
    pub fn from_error_code(error_code: &str) -> Self {
        match error_code {
            "NETWORK_TIMEOUT"
            | "WORKER_LOST"
            | "MODEL_LOAD_ERROR"
            | "CUDA_OOM"
            | "RATE_LIMITED"
            | "QUOTA_EXHAUSTED"
            | "QUOTA_TEMPORARY"
            | "PROVIDER_UNAVAILABLE"
            | "LOCAL_RUNTIME_CONTEXT_ERROR"
            | "LOCAL_EXPORT_ERROR"
            | "LOCAL_RESTART_PENDING_RECONCILIATION" => Self::Retryable,
            _ => Self::Fatal,
        }
    }
}

impl StateStore {
    pub fn create_step(
        &self,
        project_id: &str,
        step: &str,
        unit: &str,
        status: StepStatus,
        input_hash: Option<&str>,
    ) -> Result<WorkflowStep> {
        self.get_project(project_id)?;
        if step.trim().is_empty() || unit.trim().is_empty() {
            return Err(Error::InvalidTransition(
                "step and unit must not be empty".to_owned(),
            ));
        }

        let workflow_step = WorkflowStep {
            step_id: format!("step_{}", Uuid::new_v4().simple()),
            project_id: project_id.to_owned(),
            step: step.to_owned(),
            unit: unit.to_owned(),
            status,
            input_hash: input_hash.map(ToOwned::to_owned),
        };

        self.connection.execute(
            "INSERT INTO steps(id,project_id,step_key,unit_key,status,input_hash) \
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                &workflow_step.step_id,
                &workflow_step.project_id,
                &workflow_step.step,
                &workflow_step.unit,
                workflow_step.status.as_str(),
                &workflow_step.input_hash,
            ],
        )?;
        Ok(workflow_step)
    }

    pub fn get_step(&self, step_id: &str) -> Result<WorkflowStep> {
        self.connection
            .query_row(
                "SELECT id,project_id,step_key,unit_key,status,input_hash FROM steps WHERE id=?1",
                [step_id],
                step_from_row,
            )
            .optional()?
            .ok_or_else(|| Error::StepNotFound(step_id.to_owned()))
    }

    pub fn set_step_status(&self, step_id: &str, next: StepStatus) -> Result<WorkflowStep> {
        let current = self.get_step(step_id)?;
        ensure_transition(current.status, next, "step", step_id)?;
        self.connection.execute(
            "UPDATE steps SET status=?1 WHERE id=?2",
            params![next.as_str(), step_id],
        )?;
        self.get_step(step_id)
    }

    pub fn set_project_production_lock(&self, project_id: &str, locked: bool) -> Result<()> {
        let changed = self.connection.execute(
            "UPDATE projects SET production_lock=?1, updated_at=?2 WHERE id=?3",
            params![i64::from(locked), Utc::now().to_rfc3339(), project_id],
        )?;
        if changed == 0 {
            return Err(Error::ProjectNotFound(project_id.to_owned()));
        }
        Ok(())
    }

    pub fn add_dependency(&self, upstream_step_id: &str, downstream_step_id: &str) -> Result<()> {
        if upstream_step_id == downstream_step_id {
            return Err(Error::DependencyCycle(
                upstream_step_id.to_owned(),
                downstream_step_id.to_owned(),
            ));
        }

        let upstream = self.get_step(upstream_step_id)?;
        let downstream = self.get_step(downstream_step_id)?;
        if upstream.project_id != downstream.project_id {
            return Err(Error::CrossProjectDependency);
        }

        let creates_cycle: i64 = self.connection.query_row(
            r#"
            WITH RECURSIVE reachable(id) AS (
                SELECT ?1
                UNION
                SELECT d.downstream_step_id
                FROM dependencies d
                JOIN reachable r ON d.upstream_step_id = r.id
            )
            SELECT EXISTS(SELECT 1 FROM reachable WHERE id=?2)
            "#,
            params![downstream_step_id, upstream_step_id],
            |row| row.get(0),
        )?;

        if creates_cycle != 0 {
            return Err(Error::DependencyCycle(
                upstream_step_id.to_owned(),
                downstream_step_id.to_owned(),
            ));
        }

        self.connection.execute(
            "INSERT OR IGNORE INTO dependencies(upstream_step_id,downstream_step_id) VALUES (?1,?2)",
            params![upstream_step_id, downstream_step_id],
        )?;
        Ok(())
    }

    pub fn downstream_steps(&self, root_step_id: &str) -> Result<Vec<WorkflowStep>> {
        self.get_step(root_step_id)?;
        let mut statement = self.connection.prepare(
            r#"
            WITH RECURSIVE affected(id, depth) AS (
                SELECT ?1, 0
                UNION
                SELECT d.downstream_step_id, affected.depth + 1
                FROM dependencies d
                JOIN affected ON d.upstream_step_id = affected.id
            )
            SELECT s.id,s.project_id,s.step_key,s.unit_key,s.status,s.input_hash
            FROM affected
            JOIN steps s ON s.id=affected.id
            ORDER BY affected.depth,s.step_key,s.unit_key
            "#,
        )?;
        let steps = statement
            .query_map([root_step_id], step_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(steps)
    }

    pub fn preview_invalidation(&self, root_step_id: &str) -> Result<Vec<InvalidationImpact>> {
        Ok(self
            .downstream_steps(root_step_id)?
            .into_iter()
            .map(|step| InvalidationImpact {
                step_id: step.step_id,
                step: step.step,
                unit: step.unit,
                previous_status: step.status,
            })
            .collect())
    }

    pub fn invalidate_from(
        &mut self,
        root_step_id: &str,
        new_input_hash: Option<&str>,
    ) -> Result<Vec<InvalidationImpact>> {
        let root = self.get_step(root_step_id)?;
        let impact = self.preview_invalidation(root_step_id)?;
        let transaction = self.connection.transaction()?;

        if let Some(input_hash) = new_input_hash {
            transaction.execute(
                "UPDATE steps SET input_hash=?1 WHERE id=?2",
                params![input_hash, root_step_id],
            )?;
        }

        for affected in &impact {
            transaction.execute(
                "UPDATE steps SET status='STALE' \
                 WHERE id=?1 AND status NOT IN ('NOT_READY','CANCELLED','STALE')",
                [&affected.step_id],
            )?;
            transaction.execute(
                "UPDATE jobs SET status='STALE' \
                 WHERE project_id=?1 AND step_key=?2 AND unit_key=?3 \
                 AND status NOT IN ('CANCELLED','STALE')",
                params![&root.project_id, &affected.step, &affected.unit],
            )?;
        }

        transaction.commit()?;
        Ok(impact)
    }

    pub fn refresh_ready_steps(&self, project_id: &str) -> Result<usize> {
        self.get_project(project_id)?;
        let changed = self.connection.execute(
            r#"
            UPDATE steps
            SET status='READY'
            WHERE project_id=?1
              AND status='NOT_READY'
              AND NOT EXISTS (
                  SELECT 1
                  FROM dependencies d
                  JOIN steps upstream ON upstream.id=d.upstream_step_id
                  WHERE d.downstream_step_id=steps.id
                    AND upstream.status NOT IN ('SUCCEEDED','SKIPPED')
              )
            "#,
            [project_id],
        )?;
        Ok(changed)
    }

    pub fn start_attempt(&mut self, job_id: &str, worker: Option<&str>) -> Result<Attempt> {
        let job = self.get_job(job_id)?;
        ensure_transition(job.status, StepStatus::Running, "job", job_id)?;

        let attempt = Attempt {
            attempt_id: format!("attempt_{}", Uuid::new_v4().simple()),
            job_id: job_id.to_owned(),
            worker: worker.map(ToOwned::to_owned),
            started_at: Utc::now(),
            finished_at: None,
            runtime_seconds: None,
            status: StepStatus::Running,
            error_code: None,
        };

        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO attempts(id,job_id,worker,started_at,finished_at,runtime_seconds,status,error_code) \
             VALUES (?1,?2,?3,?4,NULL,NULL,'RUNNING',NULL)",
            params![
                &attempt.attempt_id,
                &attempt.job_id,
                &attempt.worker,
                attempt.started_at.to_rfc3339(),
            ],
        )?;
        transaction.execute("UPDATE jobs SET status='RUNNING' WHERE id=?1", [job_id])?;
        transaction.commit()?;
        Ok(attempt)
    }

    pub fn get_attempt(&self, attempt_id: &str) -> Result<Attempt> {
        self.connection
            .query_row(
                "SELECT id,job_id,worker,started_at,finished_at,runtime_seconds,status,error_code \
                 FROM attempts WHERE id=?1",
                [attempt_id],
                attempt_from_row,
            )
            .optional()?
            .ok_or_else(|| Error::AttemptNotFound(attempt_id.to_owned()))
    }

    pub fn list_attempts(&self, job_id: &str) -> Result<Vec<Attempt>> {
        self.get_job(job_id)?;
        let mut statement = self.connection.prepare(
            "SELECT id,job_id,worker,started_at,finished_at,runtime_seconds,status,error_code \
             FROM attempts WHERE job_id=?1 ORDER BY started_at,id",
        )?;
        let attempts = statement
            .query_map([job_id], attempt_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(attempts)
    }

    pub fn finish_attempt_success(&mut self, attempt_id: &str) -> Result<Attempt> {
        let attempt = self.get_attempt(attempt_id)?;
        ensure_transition(attempt.status, StepStatus::Succeeded, "attempt", attempt_id)?;
        let job = self.get_job(&attempt.job_id)?;
        if job.status != StepStatus::Running {
            return Err(Error::InvalidTransition(format!(
                "job {} must be RUNNING while attempt succeeds",
                job.job_id
            )));
        }

        let finished_at = Utc::now();
        let runtime_seconds = runtime_seconds(attempt.started_at, finished_at);
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE attempts SET status='SUCCEEDED',finished_at=?1,runtime_seconds=?2,error_code=NULL \
             WHERE id=?3",
            params![finished_at.to_rfc3339(), runtime_seconds, attempt_id],
        )?;
        transaction.execute(
            "UPDATE jobs SET selected_attempt_id=?1 WHERE id=?2",
            params![attempt_id, &attempt.job_id],
        )?;
        transaction.commit()?;
        self.get_attempt(attempt_id)
    }

    pub fn finish_attempt_failure(
        &mut self,
        attempt_id: &str,
        error_code: &str,
    ) -> Result<Attempt> {
        if error_code.trim().is_empty() {
            return Err(Error::InvalidJobState(
                "error_code must not be empty".to_owned(),
            ));
        }

        let attempt = self.get_attempt(attempt_id)?;
        let disposition = FailureDisposition::from_error_code(error_code);
        let next = match disposition {
            FailureDisposition::Retryable => StepStatus::Retryable,
            FailureDisposition::Fatal => StepStatus::Fatal,
        };
        ensure_transition(attempt.status, next, "attempt", attempt_id)?;

        let job = self.get_job(&attempt.job_id)?;
        ensure_transition(job.status, next, "job", &job.job_id)?;

        let finished_at = Utc::now();
        let runtime_seconds = runtime_seconds(attempt.started_at, finished_at);
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE attempts SET status=?1,finished_at=?2,runtime_seconds=?3,error_code=?4 WHERE id=?5",
            params![
                next.as_str(),
                finished_at.to_rfc3339(),
                runtime_seconds,
                error_code,
                attempt_id
            ],
        )?;
        transaction.execute(
            "UPDATE jobs SET status=?1 WHERE id=?2",
            params![next.as_str(), &attempt.job_id],
        )?;
        transaction.commit()?;
        self.get_attempt(attempt_id)
    }

    pub fn reconcile_interrupted_jobs(&mut self) -> Result<ReconciliationSummary> {
        let jobs_marked_retryable: usize = self.connection.query_row(
            "SELECT COUNT(*) FROM jobs WHERE status='RUNNING'",
            [],
            |row| row.get(0),
        )?;
        let attempts_marked_retryable: usize = self.connection.query_row(
            "SELECT COUNT(*) FROM attempts WHERE status='RUNNING'",
            [],
            |row| row.get(0),
        )?;

        if jobs_marked_retryable == 0 && attempts_marked_retryable == 0 {
            return Ok(ReconciliationSummary {
                jobs_marked_retryable: 0,
                attempts_marked_retryable: 0,
            });
        }

        let now = Utc::now();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE compute_attempt_contexts \
             SET runtime_observation_eligible=0 \
             WHERE attempt_id IN (SELECT id FROM attempts WHERE status='RUNNING')",
            [],
        )?;
        transaction.execute(
            "UPDATE attempts \
             SET status='RETRYABLE',finished_at=?1, \
                 runtime_seconds=(julianday(?1)-julianday(started_at))*86400.0, \
                 error_code=COALESCE(error_code,'LOCAL_RESTART_PENDING_RECONCILIATION') \
             WHERE status='RUNNING'",
            [now.to_rfc3339()],
        )?;
        transaction.execute(
            "UPDATE jobs SET status='RETRYABLE' WHERE status='RUNNING'",
            [],
        )?;
        transaction.commit()?;

        Ok(ReconciliationSummary {
            jobs_marked_retryable,
            attempts_marked_retryable,
        })
    }

    pub fn derive_project_status(&self, project_id: &str) -> Result<ProjectDisplayStatus> {
        self.get_project(project_id)?;

        let mut statement = self
            .connection
            .prepare("SELECT status FROM jobs WHERE project_id=?1")?;
        let job_statuses = statement
            .query_map([project_id], |row| {
                let raw: String = row.get(0)?;
                parse_step_status(&raw, 0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if !job_statuses.is_empty() {
            return Ok(derive_display_status(&job_statuses));
        }

        let mut statement = self
            .connection
            .prepare("SELECT status FROM steps WHERE project_id=?1")?;
        let step_statuses = statement
            .query_map([project_id], |row| {
                let raw: String = row.get(0)?;
                parse_step_status(&raw, 0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if step_statuses.is_empty() {
            return Ok(ProjectDisplayStatus::Draft);
        }
        Ok(derive_display_status(&step_statuses))
    }
}

impl StateStore {
    pub fn commit_attempt_artifact_success(
        &mut self,
        attempt_id: &str,
        artifact: &Artifact,
    ) -> Result<Attempt> {
        self.commit_attempt_artifacts_success(
            attempt_id,
            std::slice::from_ref(artifact),
            &artifact.artifact_id,
        )
    }

    pub fn commit_attempt_artifacts_success(
        &mut self,
        attempt_id: &str,
        artifacts: &[Artifact],
        selected_artifact_id: &str,
    ) -> Result<Attempt> {
        if artifacts.is_empty() {
            return Err(Error::InvalidArtifact(
                "attempt success requires at least one artifact".to_owned(),
            ));
        }
        if !artifacts
            .iter()
            .any(|artifact| artifact.artifact_id == selected_artifact_id)
        {
            return Err(Error::InvalidArtifact(
                "selected artifact must be part of attempt artifacts".to_owned(),
            ));
        }

        let attempt = self.get_attempt(attempt_id)?;
        ensure_transition(attempt.status, StepStatus::Succeeded, "attempt", attempt_id)?;

        let job = self.get_job(&attempt.job_id)?;
        ensure_transition(job.status, StepStatus::Succeeded, "job", &job.job_id)?;

        let mut serialized = Vec::with_capacity(artifacts.len());
        for artifact in artifacts {
            let producer_job = artifact
                .producer_job
                .as_deref()
                .ok_or_else(|| Error::InvalidArtifact("producer_job is required".to_owned()))?;
            if producer_job != job.job_id {
                return Err(Error::InvalidArtifact(
                    "artifact producer_job does not match attempt job".to_owned(),
                ));
            }

            let artifact_project_id = artifact
                .project_id
                .as_deref()
                .ok_or_else(|| Error::InvalidArtifact("project_id is required".to_owned()))?;
            if artifact_project_id != job.project_id {
                return Err(Error::InvalidArtifact(
                    "artifact project_id does not match producer job".to_owned(),
                ));
            }

            let artifact_input_hash = artifact
                .input_hash
                .as_deref()
                .ok_or_else(|| Error::InvalidArtifact("input_hash is required".to_owned()))?;
            if artifact_input_hash != job.input_hash {
                return Err(Error::InvalidArtifact(
                    "artifact input_hash does not match producer job".to_owned(),
                ));
            }

            serialized.push((artifact, serde_json::to_string(&artifact.metadata)?));
        }

        let finished_at = Utc::now();
        let runtime_seconds = runtime_seconds(attempt.started_at, finished_at);
        let transaction = self.connection.transaction()?;
        for (artifact, metadata_json) in serialized {
            transaction.execute(
                "INSERT INTO artifacts(id,project_id,artifact_type,uri,sha256,size_bytes,producer_job_id,created_at,metadata_json,input_hash) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    &artifact.artifact_id,
                    &artifact.project_id,
                    &artifact.artifact_type,
                    artifact.uri.as_str(),
                    &artifact.sha256,
                    artifact.size_bytes as i64,
                    &artifact.producer_job,
                    artifact.created_at.to_rfc3339(),
                    metadata_json,
                    &artifact.input_hash,
                ],
            )?;
        }
        transaction.execute(
            "UPDATE attempts SET status='SUCCEEDED',finished_at=?1,runtime_seconds=?2,error_code=NULL \
             WHERE id=?3 AND job_id=?4",
            params![
                finished_at.to_rfc3339(),
                runtime_seconds,
                attempt_id,
                &job.job_id
            ],
        )?;
        transaction.execute(
            "UPDATE jobs SET status='SUCCEEDED',selected_attempt_id=?1,selected_artifact_id=?2 \
             WHERE id=?3",
            params![attempt_id, selected_artifact_id, &job.job_id],
        )?;
        transaction.commit()?;

        self.get_attempt(attempt_id)
    }
}

fn ensure_transition(current: StepStatus, next: StepStatus, entity: &str, id: &str) -> Result<()> {
    if current.can_transition_to(next) {
        return Ok(());
    }
    Err(Error::InvalidTransition(format!(
        "{entity} {id}: {} -> {}",
        current.as_str(),
        next.as_str()
    )))
}

fn derive_display_status(statuses: &[StepStatus]) -> ProjectDisplayStatus {
    if statuses
        .iter()
        .all(|status| matches!(status, StepStatus::Succeeded | StepStatus::Skipped))
    {
        return ProjectDisplayStatus::ReadyForEdit;
    }
    if statuses.contains(&StepStatus::Running) {
        return ProjectDisplayStatus::GpuRunning;
    }
    if statuses
        .iter()
        .any(|status| matches!(status, StepStatus::Failed | StepStatus::Fatal))
    {
        return ProjectDisplayStatus::NeedsReview;
    }
    if statuses.contains(&StepStatus::Retryable) {
        return ProjectDisplayStatus::GpuPartial;
    }
    if statuses
        .iter()
        .any(|status| matches!(status, StepStatus::Ready | StepStatus::Queued))
    {
        return ProjectDisplayStatus::GpuReady;
    }
    ProjectDisplayStatus::Preparing
}

fn step_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkflowStep> {
    let raw_status: String = row.get(4)?;
    Ok(WorkflowStep {
        step_id: row.get(0)?,
        project_id: row.get(1)?,
        step: row.get(2)?,
        unit: row.get(3)?,
        status: parse_step_status(&raw_status, 4)?,
        input_hash: row.get(5)?,
    })
}

fn attempt_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Attempt> {
    let started_at: String = row.get(3)?;
    let finished_at: Option<String> = row.get(4)?;
    let raw_status: String = row.get(6)?;

    Ok(Attempt {
        attempt_id: row.get(0)?,
        job_id: row.get(1)?,
        worker: row.get(2)?,
        started_at: parse_time(&started_at, 3)?,
        finished_at: finished_at
            .as_deref()
            .map(|value| parse_time(value, 4))
            .transpose()?,
        runtime_seconds: row.get(5)?,
        status: parse_step_status(&raw_status, 6)?,
        error_code: row.get(7)?,
    })
}

fn parse_time(value: &str, column: usize) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(error))
        })
}

fn runtime_seconds(started_at: DateTime<Utc>, finished_at: DateTime<Utc>) -> f64 {
    finished_at
        .signed_duration_since(started_at)
        .num_milliseconds() as f64
        / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{deterministic_input_hash, Workspace};

    #[test]
    fn readiness_follows_dependencies() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let state = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = state.create_project("Ready DAG").unwrap();

        let root = state
            .create_step(&project.id, "script", "S01", StepStatus::NotReady, None)
            .unwrap();
        let downstream = state
            .create_step(&project.id, "tts", "S01", StepStatus::NotReady, None)
            .unwrap();
        state
            .add_dependency(&root.step_id, &downstream.step_id)
            .unwrap();

        assert_eq!(state.refresh_ready_steps(&project.id).unwrap(), 1);
        assert_eq!(
            state.get_step(&root.step_id).unwrap().status,
            StepStatus::Ready
        );
        assert_eq!(
            state.get_step(&downstream.step_id).unwrap().status,
            StepStatus::NotReady
        );

        state
            .set_step_status(&root.step_id, StepStatus::Succeeded)
            .unwrap();
        assert_eq!(state.refresh_ready_steps(&project.id).unwrap(), 1);
        assert_eq!(
            state.get_step(&downstream.step_id).unwrap().status,
            StepStatus::Ready
        );
    }

    #[test]
    fn invalidation_is_downstream_only_and_preview_is_non_mutating() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = state.create_project("Locked Production").unwrap();
        state
            .set_project_production_lock(&project.id, true)
            .unwrap();

        let script = state
            .create_step(
                &project.id,
                "script",
                "S04",
                StepStatus::Succeeded,
                Some("old-script"),
            )
            .unwrap();
        let tts = state
            .create_step(
                &project.id,
                "tts",
                "S04",
                StepStatus::Succeeded,
                Some("old-tts"),
            )
            .unwrap();
        let timing = state
            .create_step(
                &project.id,
                "timing",
                "S04",
                StepStatus::Succeeded,
                Some("old-timing"),
            )
            .unwrap();
        let unrelated = state
            .create_step(
                &project.id,
                "image",
                "SC99",
                StepStatus::Succeeded,
                Some("image-hash"),
            )
            .unwrap();

        state.add_dependency(&script.step_id, &tts.step_id).unwrap();
        state.add_dependency(&tts.step_id, &timing.step_id).unwrap();

        let preview = state.preview_invalidation(&script.step_id).unwrap();
        assert_eq!(preview.len(), 3);
        assert!(preview.iter().all(|item| item.unit == "S04"));
        assert_eq!(
            state.get_step(&script.step_id).unwrap().status,
            StepStatus::Succeeded
        );

        let impact = state
            .invalidate_from(&script.step_id, Some("new-script"))
            .unwrap();
        assert_eq!(impact, preview);
        assert_eq!(
            state.get_step(&script.step_id).unwrap().status,
            StepStatus::Stale
        );
        assert_eq!(
            state.get_step(&tts.step_id).unwrap().status,
            StepStatus::Stale
        );
        assert_eq!(
            state.get_step(&timing.step_id).unwrap().status,
            StepStatus::Stale
        );
        assert_eq!(
            state.get_step(&unrelated.step_id).unwrap().status,
            StepStatus::Succeeded
        );
        assert_eq!(
            state
                .get_step(&script.step_id)
                .unwrap()
                .input_hash
                .as_deref(),
            Some("new-script")
        );
        assert!(state.get_project(&project.id).unwrap().production_lock);
    }

    #[test]
    fn dependency_cycles_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let state = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = state.create_project("DAG").unwrap();

        let a = state
            .create_step(&project.id, "a", "1", StepStatus::Ready, None)
            .unwrap();
        let b = state
            .create_step(&project.id, "b", "1", StepStatus::Ready, None)
            .unwrap();
        let c = state
            .create_step(&project.id, "c", "1", StepStatus::Ready, None)
            .unwrap();

        state.add_dependency(&a.step_id, &b.step_id).unwrap();
        state.add_dependency(&b.step_id, &c.step_id).unwrap();
        assert!(matches!(
            state.add_dependency(&c.step_id, &a.step_id),
            Err(Error::DependencyCycle(_, _))
        ));
    }

    #[test]
    fn retries_create_new_attempts_and_preserve_history() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = state.create_project("Retry History").unwrap();
        let input_hash = deterministic_input_hash(&[b"voice", b"S07"]);
        let job = state
            .create_job(&project.id, "tts", "S07", &input_hash)
            .unwrap();

        assert_eq!(
            state.derive_project_status(&project.id).unwrap(),
            ProjectDisplayStatus::GpuReady
        );

        let first = state.start_attempt(&job.job_id, Some("gpu0")).unwrap();
        assert_eq!(
            state.derive_project_status(&project.id).unwrap(),
            ProjectDisplayStatus::GpuRunning
        );
        let first = state
            .finish_attempt_failure(&first.attempt_id, "NETWORK_TIMEOUT")
            .unwrap();
        assert_eq!(first.status, StepStatus::Retryable);
        assert_eq!(
            state.derive_project_status(&project.id).unwrap(),
            ProjectDisplayStatus::GpuPartial
        );

        let second = state.start_attempt(&job.job_id, Some("gpu1")).unwrap();
        let second = state.finish_attempt_success(&second.attempt_id).unwrap();
        assert_eq!(second.status, StepStatus::Succeeded);

        let attempts = state.list_attempts(&job.job_id).unwrap();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].status, StepStatus::Retryable);
        assert_eq!(attempts[1].status, StepStatus::Succeeded);

        let job = state.get_job(&job.job_id).unwrap();
        assert_eq!(job.status, StepStatus::Running);
        assert_eq!(
            job.selected_attempt.as_deref(),
            Some(second.attempt_id.as_str())
        );
    }

    #[test]
    fn startup_reconciliation_never_auto_reexecutes_running_work() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = state.create_project("Restart").unwrap();
        let job = state
            .create_job(&project.id, "tts", "S01", "input-hash")
            .unwrap();
        let attempt = state
            .start_attempt(&job.job_id, Some("remote-gpu"))
            .unwrap();

        let summary = state.reconcile_interrupted_jobs().unwrap();
        assert_eq!(summary.jobs_marked_retryable, 1);
        assert_eq!(summary.attempts_marked_retryable, 1);
        assert_eq!(
            state.get_job(&job.job_id).unwrap().status,
            StepStatus::Retryable
        );

        let reconciled_attempt = state.get_attempt(&attempt.attempt_id).unwrap();
        assert_eq!(reconciled_attempt.status, StepStatus::Retryable);
        assert_eq!(
            reconciled_attempt.error_code.as_deref(),
            Some("LOCAL_RESTART_PENDING_RECONCILIATION")
        );

        let second_pass = state.reconcile_interrupted_jobs().unwrap();
        assert_eq!(second_pass.jobs_marked_retryable, 0);
        assert_eq!(second_pass.attempts_marked_retryable, 0);
    }

    #[test]
    fn unknown_failures_are_fatal_by_default() {
        assert_eq!(
            FailureDisposition::from_error_code("BAD_INPUT"),
            FailureDisposition::Fatal
        );
        assert_eq!(
            FailureDisposition::from_error_code("WORKER_LOST"),
            FailureDisposition::Retryable
        );
        assert_eq!(
            FailureDisposition::from_error_code("SOMETHING_NEW"),
            FailureDisposition::Fatal
        );
    }
}
