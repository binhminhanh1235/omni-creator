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
    #[error("workspace is busy: {0}")]
    WorkspaceBusy(String),
    #[error("invalid machine binding: {0}")]
    InvalidMachineBinding(String),
    #[error(
        "machine binding points to a different workspace: expected {expected}, found {actual}"
    )]
    WorkspaceBindingMismatch { expected: String, actual: String },
    #[error("invalid handoff: {0}")]
    InvalidHandoff(String),
    #[error("path is not valid UTF-8: {0}")]
    InvalidPathEncoding(PathBuf),
    #[error("invalid logical URI: {0}")]
    InvalidLogicalUri(String),
    #[error("logical path escapes its allowed root: {0}")]
    PathEscape(String),
    #[error("artifact URI requires artifact-store lookup: {0}")]
    ArtifactResolutionRequired(String),
    #[error("artifact target already exists: {0}")]
    ArtifactTargetExists(PathBuf),
    #[error("artifact hash verification failed for {0}")]
    ArtifactHashMismatch(String),
    #[error("invalid artifact: {0}")]
    InvalidArtifact(String),
    #[error("artifact not found: {0}")]
    ArtifactNotFound(String),
    #[error("project not found: {0}")]
    ProjectNotFound(String),
    #[error("job not found: {0}")]
    JobNotFound(String),
    #[error("step not found: {0}")]
    StepNotFound(String),
    #[error("attempt not found: {0}")]
    AttemptNotFound(String),
    #[error("invalid job state: {0}")]
    InvalidJobState(String),
    #[error("invalid workflow transition: {0}")]
    InvalidTransition(String),
    #[error("invalid contract: {0}")]
    InvalidContract(String),
    #[error("invalid LLMGateway configuration: {0}")]
    InvalidLlmGatewayConfig(String),
    #[error("LLMGateway credential is unavailable in environment variable {0}")]
    MissingLlmGatewayCredential(String),
    #[error("LLMGateway transport error: {0}")]
    LlmGatewayTransport(String),
    #[error("LLMGateway API error HTTP {status}: {message}")]
    LlmGatewayApi {
        status: u16,
        code: Option<String>,
        message: String,
    },
    #[error("invalid LLMGateway response: {0}")]
    InvalidLlmGatewayResponse(String),
    #[error("dependency would create a cycle: {0} -> {1}")]
    DependencyCycle(String, String),
    #[error("workflow dependency crosses project boundaries")]
    CrossProjectDependency,
}

pub type Result<T> = std::result::Result<T, Error>;
