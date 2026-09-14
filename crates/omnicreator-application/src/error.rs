use std::fmt;

use omnicreator_core::Error as CoreError;
use serde::{Deserialize, Serialize};

pub const CONTROL_ERROR_SCHEMA_V1: &str = "omnicreator.control-error";
pub const CONTROL_ERROR_VERSION_V1: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControlErrorCodeV1 {
    ReadOnly,
    WriterConflict,
    NotFound,
    InvalidInput,
    InvalidTransition,
    Blocked,
    CapabilityUnavailable,
    ProviderUnavailable,
    ArtifactInvalid,
    StaleInput,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ControlErrorV1 {
    pub schema: String,
    pub version: u32,
    pub code: ControlErrorCodeV1,
    pub message: String,
}

impl ControlErrorV1 {
    pub fn new(code: ControlErrorCodeV1, message: impl Into<String>) -> Self {
        Self {
            schema: CONTROL_ERROR_SCHEMA_V1.to_owned(),
            version: CONTROL_ERROR_VERSION_V1,
            code,
            message: message.into(),
        }
    }

    pub fn read_only() -> Self {
        Self::new(
            ControlErrorCodeV1::ReadOnly,
            "the active workspace session is read-only",
        )
    }
}

impl fmt::Display for ControlErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for ControlErrorV1 {}

impl From<CoreError> for ControlErrorV1 {
    fn from(error: CoreError) -> Self {
        let message = error.to_string();
        let code = match &error {
            CoreError::WorkspaceBusy(_) => ControlErrorCodeV1::WriterConflict,
            CoreError::ProjectNotFound(_)
            | CoreError::JobNotFound(_)
            | CoreError::StepNotFound(_)
            | CoreError::AttemptNotFound(_)
            | CoreError::ArtifactNotFound(_) => ControlErrorCodeV1::NotFound,
            CoreError::InvalidTransition(_) | CoreError::InvalidJobState(_) => {
                ControlErrorCodeV1::InvalidTransition
            }
            CoreError::ArtifactHashMismatch(_)
            | CoreError::InvalidArtifact(_)
            | CoreError::ArtifactResolutionRequired(_)
            | CoreError::ArtifactTargetExists(_)
            | CoreError::ExportArtifactProjectMismatch { .. }
            | CoreError::ExportArtifactUriMismatch { .. }
            | CoreError::ExportArtifactFileMissing { .. } => ControlErrorCodeV1::ArtifactInvalid,
            CoreError::MissingComputeProviderCredential(_)
            | CoreError::ComputeProviderTransport(_)
            | CoreError::ComputeProviderApi { .. }
            | CoreError::MissingLlmProviderCredential { .. }
            | CoreError::LlmProviderTransport { .. }
            | CoreError::LlmProviderApi { .. }
            | CoreError::MissingLlmGatewayCredential(_)
            | CoreError::LlmGatewayTransport(_)
            | CoreError::LlmGatewayApi { .. }
            | CoreError::PluginSpawn { .. }
            | CoreError::PluginTimeout { .. }
            | CoreError::PluginProcessExited { .. }
            | CoreError::PluginRuntimeIo { .. } => ControlErrorCodeV1::ProviderUnavailable,
            CoreError::InvalidComputeProviderConfig(_)
            | CoreError::InvalidComputeProviderResponse(_)
            | CoreError::InvalidLlmProviderResponse { .. }
            | CoreError::PluginProtocol { .. } => ControlErrorCodeV1::CapabilityUnavailable,
            CoreError::InvalidContract(reason) if reason.to_ascii_lowercase().contains("stale") => {
                ControlErrorCodeV1::StaleInput
            }
            CoreError::InvalidContract(_)
            | CoreError::InvalidWorkspace(_)
            | CoreError::WorkspaceAlreadyExists(_)
            | CoreError::InvalidMachineBinding(_)
            | CoreError::WorkspaceBindingMismatch { .. }
            | CoreError::InvalidHandoff(_)
            | CoreError::InvalidPathEncoding(_)
            | CoreError::InvalidLogicalUri(_)
            | CoreError::PathEscape(_)
            | CoreError::InvalidExportPath(_)
            | CoreError::InvalidLlmProviderConfig(_)
            | CoreError::InvalidLlmGatewayConfig(_)
            | CoreError::InvalidLlmGatewayResponse(_)
            | CoreError::InvalidStructuredOutput { .. }
            | CoreError::DependencyCycle(_, _)
            | CoreError::CrossProjectDependency => ControlErrorCodeV1::InvalidInput,
            CoreError::Io(_) | CoreError::Json(_) | CoreError::Sqlite(_) => {
                ControlErrorCodeV1::Internal
            }
        };
        Self::new(code, message)
    }
}

pub type ControlResultV1<T> = Result<T, ControlErrorV1>;