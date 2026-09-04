use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("invalid workspace: {0}")]
    InvalidWorkspace(String),
    #[error("workspace already exists at {0}")]
    WorkspaceAlreadyExists(PathBuf),
    #[error("invalid logical URI: {0}")]
    InvalidLogicalUri(String),
    #[error("logical path escapes its allowed root: {0}")]
    PathEscape(String),
    #[error("artifact URI requires artifact-store lookup: {0}")]
    ArtifactResolutionRequired(String),
    #[error("project not found: {0}")]
    ProjectNotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;
