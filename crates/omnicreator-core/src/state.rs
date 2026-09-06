use std::{io, path::Path};

use chrono::{DateTime, Utc};
use rusqlite::{params, types::Type, Connection, OpenFlags, OptionalExtension};
use uuid::Uuid;

use crate::{Artifact, Error, Job, LogicalUri, Project, Result, StepStatus};

const MIGRATION_V1: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    studio_pack TEXT,
    channel_profile TEXT,
    script_version INTEGER NOT NULL DEFAULT 1,
    production_lock INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS steps (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    step_key TEXT NOT NULL,
    unit_key TEXT NOT NULL,
    status TEXT NOT NULL,
    input_hash TEXT,
    UNIQUE(project_id, step_key, unit_key)
);

CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    step_key TEXT NOT NULL,
    unit_key TEXT NOT NULL,
    status TEXT NOT NULL,
    input_hash TEXT NOT NULL,
    selected_attempt_id TEXT
);

CREATE TABLE IF NOT EXISTS attempts (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    worker TEXT,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    runtime_seconds REAL,
    status TEXT NOT NULL,
    error_code TEXT
);

CREATE TABLE IF NOT EXISTS artifacts (
    id TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    artifact_type TEXT NOT NULL,
    uri TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    producer_job_id TEXT REFERENCES jobs(id),
    created_at TEXT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS dependencies (
    upstream_step_id TEXT NOT NULL REFERENCES steps(id) ON DELETE CASCADE,
    downstream_step_id TEXT NOT NULL REFERENCES steps(id) ON DELETE CASCADE,
    PRIMARY KEY(upstream_step_id, downstream_step_id)
);
"#;

const MIGRATION_V2: &str = r#"
ALTER TABLE artifacts ADD COLUMN input_hash TEXT;
ALTER TABLE jobs ADD COLUMN selected_artifact_id TEXT;
CREATE INDEX IF NOT EXISTS idx_artifacts_input_hash ON artifacts(input_hash);
CREATE INDEX IF NOT EXISTS idx_jobs_input_hash ON jobs(input_hash);
"#;

const MIGRATION_V3: &str = r#"
CREATE INDEX IF NOT EXISTS idx_steps_project ON steps(project_id);
CREATE INDEX IF NOT EXISTS idx_dependencies_downstream ON dependencies(downstream_step_id);
CREATE INDEX IF NOT EXISTS idx_attempts_job ON attempts(job_id);
"#;

const MIGRATION_V4: &str = r#"
CREATE TABLE IF NOT EXISTS compute_attempt_contexts (
    attempt_id TEXT PRIMARY KEY REFERENCES attempts(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    plugin_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    model_version TEXT NOT NULL,
    runtime_observation_eligible INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS compute_runtime_samples (
    attempt_id TEXT PRIMARY KEY REFERENCES attempts(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    plugin_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    model_version TEXT NOT NULL,
    runtime_seconds REAL NOT NULL,
    observed_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS compute_runtime_estimates (
    provider_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    plugin_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    model_version TEXT NOT NULL,
    sample_count INTEGER NOT NULL,
    total_runtime_seconds REAL NOT NULL,
    mean_runtime_seconds REAL NOT NULL,
    ema_runtime_seconds REAL NOT NULL,
    last_runtime_seconds REAL NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY(provider_id,device_id,plugin_id,model_id,model_version)
);

CREATE INDEX IF NOT EXISTS idx_compute_runtime_samples_key
ON compute_runtime_samples(provider_id,device_id,plugin_id,model_id,model_version,observed_at);

CREATE INDEX IF NOT EXISTS idx_compute_attempt_contexts_key
ON compute_attempt_contexts(provider_id,device_id,plugin_id,model_id,model_version);
"#;

const MIGRATION_V5: &str = r#"
CREATE TABLE IF NOT EXISTS voice_takes (
    attempt_id TEXT PRIMARY KEY REFERENCES attempts(id) ON DELETE CASCADE,
    job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    take_index INTEGER NOT NULL,
    input_hash TEXT NOT NULL,
    artifact_id TEXT UNIQUE REFERENCES artifacts(id),
    created_at TEXT NOT NULL,
    UNIQUE(job_id,take_index)
);

CREATE TABLE IF NOT EXISTS voice_retake_requests (
    job_id TEXT PRIMARY KEY REFERENCES jobs(id) ON DELETE CASCADE,
    input_hash TEXT NOT NULL,
    requested_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_voice_takes_job
ON voice_takes(job_id,take_index);

CREATE INDEX IF NOT EXISTS idx_voice_takes_input_hash
ON voice_takes(input_hash,artifact_id);
"#;

const MIGRATION_V6: &str = r#"
CREATE TABLE IF NOT EXISTS voice_take_timing_artifacts (
    attempt_id TEXT PRIMARY KEY REFERENCES voice_takes(attempt_id) ON DELETE CASCADE,
    artifact_id TEXT NOT NULL UNIQUE REFERENCES artifacts(id),
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_voice_take_timing_artifacts_artifact
ON voice_take_timing_artifacts(artifact_id);
"#;

const MIGRATION_V7: &str = r#"
CREATE TABLE IF NOT EXISTS compute_weekly_budgets (
    provider_id TEXT PRIMARY KEY,
    allowance_seconds REAL NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS compute_session_usage (
    provider_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    connected_at TEXT NOT NULL,
    finished_at TEXT,
    PRIMARY KEY(provider_id,session_id)
);

CREATE INDEX IF NOT EXISTS idx_compute_session_usage_window
ON compute_session_usage(provider_id,connected_at,finished_at);
"#;

const MIGRATION_V8: &str = r#"
CREATE INDEX IF NOT EXISTS idx_artifacts_sha256
ON artifacts(sha256);

CREATE TABLE IF NOT EXISTS artifact_tags (
    artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(artifact_id,tag)
);

CREATE INDEX IF NOT EXISTS idx_artifact_tags_tag
ON artifact_tags(tag,artifact_id);

CREATE TABLE IF NOT EXISTS artifact_usages (
    artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    usage_kind TEXT NOT NULL,
    usage_key TEXT NOT NULL,
    used_at TEXT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY(artifact_id,usage_kind,usage_key)
);

CREATE INDEX IF NOT EXISTS idx_artifact_usages_artifact_time
ON artifact_usages(artifact_id,used_at DESC);

CREATE INDEX IF NOT EXISTS idx_artifact_usages_project_time
ON artifact_usages(project_id,used_at DESC);
"#;


pub struct StateStore {
    pub(crate) connection: Connection,
}

impl StateStore {
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;\n\
             PRAGMA query_only = ON;",
        )?;
        integrity_check_connection(&connection)?;
        Ok(Self { connection })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;\n\
             PRAGMA journal_mode = DELETE;\n\
             PRAGMA synchronous = FULL;",
        )?;
        let store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
            "#,
        )?;
        self.apply_migration(1, MIGRATION_V1)?;
        self.apply_migration(2, MIGRATION_V2)?;
        self.apply_migration(3, MIGRATION_V3)?;
        self.apply_migration(4, MIGRATION_V4)?;
        self.apply_migration(5, MIGRATION_V5)?;
        self.apply_migration(6, MIGRATION_V6)?;
        self.apply_migration(7, MIGRATION_V7)?;
        self.apply_migration(8, MIGRATION_V8)?;
        Ok(())
    }

    fn apply_migration(&self, version: i64, sql: &str) -> Result<()> {
        let applied: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version=?1",
            [version],
            |row| row.get(0),
        )?;
        if applied != 0 {
            return Ok(());
        }

        self.connection.execute_batch("BEGIN IMMEDIATE")?;
        if let Err(error) = self.connection.execute_batch(sql) {
            let _ = self.connection.execute_batch("ROLLBACK");
            return Err(error.into());
        }
        if let Err(error) = self.connection.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
            params![version, Utc::now().to_rfc3339()],
        ) {
            let _ = self.connection.execute_batch("ROLLBACK");
            return Err(error.into());
        }
        self.connection.execute_batch("COMMIT")?;
        Ok(())
    }

    pub fn integrity_check(&self) -> Result<()> {
        integrity_check_connection(&self.connection)
    }

    pub fn create_snapshot(&self, destination: impl AsRef<Path>) -> Result<()> {
        self.integrity_check()?;
        let destination = destination.as_ref();
        if destination.exists() {
            return Err(Error::InvalidHandoff(format!(
                "snapshot destination already exists: {}",
                destination.display()
            )));
        }
        let destination_str = destination
            .to_str()
            .ok_or_else(|| Error::InvalidPathEncoding(destination.to_path_buf()))?;

        self.connection
            .execute("VACUUM INTO ?1", params![destination_str])?;
        Self::validate_database(destination)
    }

    pub fn validate_database(path: impl AsRef<Path>) -> Result<()> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        integrity_check_connection(&connection)
    }

    pub fn create_project(&self, title: &str) -> Result<Project> {
        self.create_project_with_studio_pack(title, None)
    }

    pub fn create_project_with_studio_pack(
        &self,
        title: &str,
        studio_pack: Option<&str>,
    ) -> Result<Project> {
        if studio_pack.is_some_and(|value| value.trim().is_empty()) {
            return Err(Error::InvalidContract(
                "project studio_pack must not be empty when present".to_owned(),
            ));
        }
        let now = Utc::now();
        let project = Project {
            id: format!("prj_{}", Uuid::new_v4().simple()),
            title: title.to_owned(),
            created_at: now,
            updated_at: now,
            studio_pack: studio_pack.map(ToOwned::to_owned),
            channel_profile: None,
            script_version: 1,
            production_lock: false,
        };

        self.connection.execute(
            "INSERT INTO projects(id,title,created_at,updated_at,studio_pack,channel_profile,script_version,production_lock)\
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                &project.id,
                &project.title,
                project.created_at.to_rfc3339(),
                project.updated_at.to_rfc3339(),
                &project.studio_pack,
                &project.channel_profile,
                project.script_version,
                i64::from(project.production_lock),
            ],
        )?;
        Ok(project)
    }

    pub fn update_project_studio_pack(
        &self,
        id: &str,
        studio_pack: Option<&str>,
    ) -> Result<Project> {
        if studio_pack.is_some_and(|value| value.trim().is_empty()) {
            return Err(Error::InvalidContract(
                "project studio_pack must not be empty when present".to_owned(),
            ));
        }
        let changed = self.connection.execute(
            "UPDATE projects SET studio_pack=?1, updated_at=?2 WHERE id=?3",
            params![studio_pack, Utc::now().to_rfc3339(), id],
        )?;
        if changed == 0 {
            return Err(Error::ProjectNotFound(id.to_owned()));
        }
        self.get_project(id)
    }

    pub fn update_project_title(&self, id: &str, title: &str) -> Result<Project> {
        let changed = self.connection.execute(
            "UPDATE projects SET title=?1, updated_at=?2 WHERE id=?3",
            params![title, Utc::now().to_rfc3339(), id],
        )?;
        if changed == 0 {
            return Err(Error::ProjectNotFound(id.to_owned()));
        }
        self.get_project(id)
    }

    pub fn delete_project(&self, id: &str) -> Result<()> {
        let changed = self
            .connection
            .execute("DELETE FROM projects WHERE id=?1", [id])?;
        if changed == 0 {
            return Err(Error::ProjectNotFound(id.to_owned()));
        }
        Ok(())
    }

    pub fn get_project(&self, id: &str) -> Result<Project> {
        self.connection
            .query_row(
                "SELECT id,title,created_at,updated_at,studio_pack,channel_profile,script_version,production_lock FROM projects WHERE id=?1",
                [id],
                project_from_row,
            )
            .optional()?
            .ok_or_else(|| Error::ProjectNotFound(id.to_owned()))
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self.connection.prepare(
            "SELECT id,title,created_at,updated_at,studio_pack,channel_profile,script_version,production_lock FROM projects ORDER BY created_at",
        )?;
        let projects = stmt
            .query_map([], project_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(projects)
    }

    pub fn create_job(
        &self,
        project_id: &str,
        step: &str,
        unit: &str,
        input_hash: &str,
    ) -> Result<Job> {
        self.get_project(project_id)?;
        if input_hash.is_empty() {
            return Err(Error::InvalidJobState(
                "input_hash must not be empty".to_owned(),
            ));
        }

        let job = Job {
            job_id: format!("job_{}", Uuid::new_v4().simple()),
            project_id: project_id.to_owned(),
            step: step.to_owned(),
            unit: unit.to_owned(),
            status: StepStatus::Ready,
            input_hash: input_hash.to_owned(),
            selected_attempt: None,
            selected_artifact: None,
        };
        self.connection.execute(
            "INSERT INTO jobs(id,project_id,step_key,unit_key,status,input_hash,selected_attempt_id,selected_artifact_id) \
             VALUES (?1,?2,?3,?4,?5,?6,NULL,NULL)",
            params![
                &job.job_id,
                &job.project_id,
                &job.step,
                &job.unit,
                job.status.as_str(),
                &job.input_hash,
            ],
        )?;
        Ok(job)
    }

    pub fn get_job(&self, id: &str) -> Result<Job> {
        self.connection
            .query_row(
                "SELECT id,project_id,step_key,unit_key,status,input_hash,selected_attempt_id,selected_artifact_id \
                 FROM jobs WHERE id=?1",
                [id],
                job_from_row,
            )
            .optional()?
            .ok_or_else(|| Error::JobNotFound(id.to_owned()))
    }

    pub fn get_artifact(&self, id: &str) -> Result<Artifact> {
        self.connection
            .query_row(
                "SELECT id,project_id,artifact_type,uri,sha256,size_bytes,producer_job_id,created_at,metadata_json,input_hash \
                 FROM artifacts WHERE id=?1",
                [id],
                artifact_from_row,
            )
            .optional()?
            .ok_or_else(|| Error::ArtifactNotFound(id.to_owned()))
    }

    pub fn find_cached_artifact(&self, input_hash: &str) -> Result<Option<Artifact>> {
        self.connection
            .query_row(
                "SELECT a.id,a.project_id,a.artifact_type,a.uri,a.sha256,a.size_bytes,a.producer_job_id,a.created_at,a.metadata_json,a.input_hash \
                 FROM artifacts a \
                 LEFT JOIN jobs j ON j.id=a.producer_job_id \
                 WHERE a.input_hash=?1 AND (a.producer_job_id IS NULL OR j.status='SUCCEEDED') \
                 ORDER BY a.created_at DESC LIMIT 1",
                [input_hash],
                artifact_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn find_cached_artifacts(&self, input_hash: &str) -> Result<Vec<Artifact>> {
        let producer_job: Option<String> = self
            .connection
            .query_row(
                "SELECT a.producer_job_id \
                 FROM artifacts a \
                 JOIN jobs j ON j.id=a.producer_job_id \
                 WHERE a.input_hash=?1 AND j.status='SUCCEEDED' AND a.producer_job_id IS NOT NULL \
                 ORDER BY a.created_at DESC,a.id DESC LIMIT 1",
                [input_hash],
                |row| row.get(0),
            )
            .optional()?;
        let Some(producer_job) = producer_job else {
            return Ok(Vec::new());
        };

        let mut statement = self.connection.prepare(
            "SELECT id,project_id,artifact_type,uri,sha256,size_bytes,producer_job_id,created_at,metadata_json,input_hash \
             FROM artifacts \
             WHERE producer_job_id=?1 AND input_hash=?2 \
             ORDER BY uri,id",
        )?;
        let artifacts = statement
            .query_map(params![producer_job, input_hash], artifact_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(artifacts)
    }

    pub fn commit_job_success(&mut self, artifact: &Artifact) -> Result<()> {
        let job_id = artifact
            .producer_job
            .as_deref()
            .ok_or_else(|| Error::InvalidArtifact("producer_job is required".to_owned()))?;
        let artifact_project_id = artifact
            .project_id
            .as_deref()
            .ok_or_else(|| Error::InvalidArtifact("project_id is required".to_owned()))?;
        let artifact_input_hash = artifact
            .input_hash
            .as_deref()
            .ok_or_else(|| Error::InvalidArtifact("input_hash is required".to_owned()))?;
        let metadata_json = serde_json::to_string(&artifact.metadata)?;

        let transaction = self.connection.transaction()?;
        let job = transaction
            .query_row(
                "SELECT id,project_id,step_key,unit_key,status,input_hash,selected_attempt_id,selected_artifact_id \
                 FROM jobs WHERE id=?1",
                [job_id],
                job_from_row,
            )
            .optional()?
            .ok_or_else(|| Error::JobNotFound(job_id.to_owned()))?;

        if job.project_id != artifact_project_id {
            return Err(Error::InvalidArtifact(
                "artifact project_id does not match producer job".to_owned(),
            ));
        }
        if job.input_hash != artifact_input_hash {
            return Err(Error::InvalidArtifact(
                "artifact input_hash does not match producer job".to_owned(),
            ));
        }
        if !matches!(
            job.status,
            StepStatus::Ready
                | StepStatus::Queued
                | StepStatus::Running
                | StepStatus::Failed
                | StepStatus::Retryable
        ) {
            return Err(Error::InvalidJobState(format!(
                "job {} cannot succeed from {}",
                job.job_id,
                job.status.as_str()
            )));
        }

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

        transaction.execute(
            "UPDATE jobs SET status='SUCCEEDED', selected_artifact_id=?1 WHERE id=?2",
            params![&artifact.artifact_id, job_id],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

fn integrity_check_connection(connection: &Connection) -> Result<()> {
    let result: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if result != "ok" {
        return Err(Error::InvalidHandoff(format!(
            "SQLite integrity_check failed: {result}"
        )));
    }
    Ok(())
}

fn project_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    let created_at: String = row.get(2)?;
    let updated_at: String = row.get(3)?;
    Ok(Project {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at: parse_time(created_at, 2)?,
        updated_at: parse_time(updated_at, 3)?,
        studio_pack: row.get(4)?,
        channel_profile: row.get(5)?,
        script_version: row.get(6)?,
        production_lock: row.get::<_, i64>(7)? != 0,
    })
}

pub(crate) fn job_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Job> {
    let status: String = row.get(4)?;
    Ok(Job {
        job_id: row.get(0)?,
        project_id: row.get(1)?,
        step: row.get(2)?,
        unit: row.get(3)?,
        status: parse_step_status(&status, 4)?,
        input_hash: row.get(5)?,
        selected_attempt: row.get(6)?,
        selected_artifact: row.get(7)?,
    })
}

pub(crate) fn artifact_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Artifact> {
    let uri: String = row.get(3)?;
    let created_at: String = row.get(7)?;
    let metadata_json: String = row.get(8)?;
    let size_bytes: i64 = row.get(5)?;

    Ok(Artifact {
        artifact_id: row.get(0)?,
        project_id: row.get(1)?,
        artifact_type: row.get(2)?,
        uri: LogicalUri::parse(&uri).map_err(|error| conversion_error(3, error))?,
        sha256: row.get(4)?,
        size_bytes: u64::try_from(size_bytes).map_err(|error| conversion_error(5, error))?,
        producer_job: row.get(6)?,
        created_at: parse_time(created_at, 7)?,
        metadata: serde_json::from_str(&metadata_json)
            .map_err(|error| conversion_error(8, error))?,
        input_hash: row.get(9)?,
    })
}

pub(crate) fn parse_step_status(value: &str, column: usize) -> rusqlite::Result<StepStatus> {
    let status = match value {
        "NOT_READY" => StepStatus::NotReady,
        "READY" => StepStatus::Ready,
        "QUEUED" => StepStatus::Queued,
        "RUNNING" => StepStatus::Running,
        "SUCCEEDED" => StepStatus::Succeeded,
        "FAILED" => StepStatus::Failed,
        "RETRYABLE" => StepStatus::Retryable,
        "FATAL" => StepStatus::Fatal,
        "STALE" => StepStatus::Stale,
        "SKIPPED" => StepStatus::Skipped,
        "CANCELLED" => StepStatus::Cancelled,
        other => {
            return Err(conversion_error(
                column,
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown step status {other}"),
                ),
            ));
        }
    };
    Ok(status)
}

fn parse_time(value: String, column: usize) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| conversion_error(column, error))
}

fn conversion_error(
    column: usize,
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workspace;

    #[test]
    fn workspace_can_move_and_project_state_survives() {
        let parent = tempfile::tempdir().unwrap();
        let source = parent.path().join("source");
        let target = parent.path().join("moved");

        let workspace = Workspace::create(&source).unwrap();
        let workspace_id = workspace.manifest().workspace_id.clone();
        let store = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = store.create_project("Portable Project").unwrap();
        drop(store);
        drop(workspace);

        std::fs::rename(&source, &target).unwrap();

        let reopened = Workspace::open(&target).unwrap();
        assert_eq!(reopened.manifest().workspace_id, workspace_id);

        let reopened_store = StateStore::open(reopened.sqlite_path()).unwrap();
        let loaded = reopened_store.get_project(&project.id).unwrap();
        assert_eq!(loaded.title, "Portable Project");
    }

    #[test]
    fn read_only_store_can_list_projects_but_cannot_mutate() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let writable = StateStore::open(workspace.sqlite_path()).unwrap();
        writable.create_project("Read Only").unwrap();
        drop(writable);

        let read_only = StateStore::open_read_only(workspace.sqlite_path()).unwrap();
        assert_eq!(read_only.list_projects().unwrap().len(), 1);
        assert!(read_only.create_project("Forbidden").is_err());
    }

    #[test]
    fn project_studio_pack_binding_persists_canonically() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let store = StateStore::open(workspace.sqlite_path()).unwrap();

        let project = store
            .create_project_with_studio_pack("Pack Project", Some("christian-cinematic"))
            .unwrap();
        assert_eq!(project.studio_pack.as_deref(), Some("christian-cinematic"));

        let updated = store
            .update_project_studio_pack(&project.id, Some("night-devotional"))
            .unwrap();
        assert_eq!(updated.studio_pack.as_deref(), Some("night-devotional"));

        drop(store);
        let reopened = StateStore::open(workspace.sqlite_path()).unwrap();
        assert_eq!(
            reopened
                .get_project(&project.id)
                .unwrap()
                .studio_pack
                .as_deref(),
            Some("night-devotional")
        );
    }

    #[test]
    fn project_update_and_delete_are_persisted() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let store = StateStore::open(workspace.sqlite_path()).unwrap();

        let project = store.create_project("Draft title").unwrap();
        let updated = store
            .update_project_title(&project.id, "Production title")
            .unwrap();
        assert_eq!(updated.title, "Production title");

        store.delete_project(&project.id).unwrap();
        assert!(matches!(
            store.get_project(&project.id),
            Err(Error::ProjectNotFound(_))
        ));
    }
}
