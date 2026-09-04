use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use uuid::Uuid;

use crate::{Error, Project, Result};

pub struct StateStore {
    connection: Connection,
}

impl StateStore {
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
            "#,
        )?;
        self.connection.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (1, ?1)",
            [Utc::now().to_rfc3339()],
        )?;
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
        let now = Utc::now();
        let project = Project {
            id: format!("prj_{}", Uuid::new_v4().simple()),
            title: title.to_owned(),
            created_at: now,
            updated_at: now,
            studio_pack: None,
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

fn parse_time(value: String, column: usize) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                column,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
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
}
