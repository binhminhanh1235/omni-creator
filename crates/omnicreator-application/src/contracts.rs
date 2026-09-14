use std::path::PathBuf;

use omnicreator_core::{
    CreatorRunCoordinatorV1, ExternalGeneratedVisualRequestV1, ExternalVoiceRequestV1, Job,
    LlmProviderKindV1, ManualResultProvenanceV1, ManualScenePlanDraftV1,
    ProductionRecoveryViewV1, Project, ProjectBoardProjectionV1, ProjectDisplayStatus,
    StudioReviewCenterV1, VoiceTimingV1, WorkflowStep, WorkflowStepExecutionPolicyV1,
};
use serde::{Deserialize, Serialize};

pub const CONTROL_CONTRACT_SCHEMA_V1: &str = "omnicreator.application-control";
pub const CONTROL_CONTRACT_VERSION_V1: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControlAccessV1 {
    ReadOnly,
    Writable,
}

impl ControlAccessV1 {
    pub fn is_read_only(self) -> bool {
        self == Self::ReadOnly
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControlOperationV1 {
    WorkspaceStatus,
    ListProjects,
    ProjectStatus,
    CreateProject,
    CreateCreatorProject,
    RenameProject,
    DeleteProject,
    BindStudioPack,
    WorkflowExecutionPolicies,
    SetWorkflowAutomaticExecution,
    CreatorRunState,
    ReviewCenter,
    ProvideManualContent,
    ImportManualContent,
    ProvideManualScenePlan,
    ImportManualScenePlan,
    ProvideManualVisual,
    ChooseAssetLibraryVisual,
    SelectStockCandidate,
    ApproveGeneratedVisual,
    PrepareExternalVisual,
    ProvideExternalVisual,
    ProvideManualVoice,
    ReplaceVoiceTiming,
    PrepareExternalVoice,
    ProvideExternalVoice,
    InspectProductionRecovery,
    RepairProductionVisual,
    RepairProductionAudio,
    RepairProductionTiming,
    RepairProductionVoiceBundle,
    AssembleProductionPack,
    RebuildAndExportProduction,
    PluginRuntimeInspection,
    ComputeRuntimeInspection,
    LlmRuntimeInspection,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ControlResponseV1<T> {
    pub schema: String,
    pub version: u32,
    pub operation: ControlOperationV1,
    pub data: T,
}

impl<T> ControlResponseV1<T> {
    pub fn new(operation: ControlOperationV1, data: T) -> Self {
        Self {
            schema: CONTROL_CONTRACT_SCHEMA_V1.to_owned(),
            version: CONTROL_CONTRACT_VERSION_V1,
            operation,
            data,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceControlSnapshotV1 {
    pub schema: String,
    pub version: u32,
    pub workspace_id: String,
    pub revision: u64,
    pub access: ControlAccessV1,
    pub read_only: bool,
    pub project_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectControlSnapshotV1 {
    pub schema: String,
    pub version: u32,
    pub project: Project,
    pub status: ProjectDisplayStatus,
    pub board: ProjectBoardProjectionV1,
    pub steps: Vec<WorkflowStep>,
    pub jobs: Vec<Job>,
    pub execution_policies: Vec<WorkflowStepExecutionPolicyV1>,
    pub run_coordinator: Option<CreatorRunCoordinatorV1>,
    pub review_center: StudioReviewCenterV1,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectListControlSnapshotV1 {
    pub schema: String,
    pub version: u32,
    pub read_only: bool,
    pub projects: Vec<ProjectControlSnapshotV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreateProjectRequestV1 {
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RenameProjectRequestV1 {
    pub project_id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectIdRequestV1 {
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BindStudioPackRequestV1 {
    pub project_id: String,
    pub studio_pack_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SetWorkflowAutomaticExecutionRequestV1 {
    pub step_id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManualContentRequestV1 {
    pub project_id: String,
    pub script: String,
    pub provenance: ManualResultProvenanceV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManualContentImportRequestV1 {
    pub project_id: String,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ManualScenePlanRequestV1 {
    pub project_id: String,
    pub draft: ManualScenePlanDraftV1,
    pub provenance: ManualResultProvenanceV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManualScenePlanImportRequestV1 {
    pub project_id: String,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManualVisualRequestV1 {
    pub project_id: String,
    pub scene_id: String,
    pub source_path: PathBuf,
    pub provenance: ManualResultProvenanceV1,
    pub replace_existing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AssetLibraryVisualRequestV1 {
    pub project_id: String,
    pub scene_id: String,
    pub artifact_id: String,
    pub replace_existing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExternalVisualResultRequestV1 {
    pub request: ExternalGeneratedVisualRequestV1,
    pub source_path: PathBuf,
    pub source_label: String,
    pub replace_existing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ManualVoiceRequestV1 {
    pub project_id: String,
    pub segment_id: String,
    pub audio_path: PathBuf,
    pub timing: VoiceTimingV1,
    pub provenance: ManualResultProvenanceV1,
    pub replace_existing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReplaceVoiceTimingRequestV1 {
    pub project_id: String,
    pub segment_id: String,
    pub timing: VoiceTimingV1,
    pub provenance: ManualResultProvenanceV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExternalVoiceResultRequestV1 {
    pub request: ExternalVoiceRequestV1,
    pub audio_path: PathBuf,
    pub timing: VoiceTimingV1,
    pub source_label: String,
    pub replace_existing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecoveryFileRequestV1 {
    pub project_id: String,
    pub canonical_id: String,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecoveryVoiceBundleRequestV1 {
    pub project_id: String,
    pub segment_id: String,
    pub audio_path: PathBuf,
    pub timing_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginRuntimeControlItemV1 {
    pub plugin_id: String,
    pub enabled: bool,
    pub status: String,
    pub capabilities: Vec<String>,
    pub reason_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginRuntimeControlSnapshotV1 {
    pub schema: String,
    pub version: u32,
    pub plugins: Vec<PluginRuntimeControlItemV1>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputeRuntimeControlSnapshotV1 {
    pub schema: String,
    pub version: u32,
    pub provider_id: String,
    pub state: String,
    pub capabilities: Vec<String>,
    pub reason_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LlmRuntimeControlSnapshotV1 {
    pub schema: String,
    pub version: u32,
    pub provider_id: Option<String>,
    pub provider_kind: Option<LlmProviderKindV1>,
    pub state: String,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
    pub default_model: Option<String>,
    pub credential_present: bool,
    pub model_discovery_supported: bool,
    pub reason_code: Option<String>,
}

impl LlmRuntimeControlSnapshotV1 {
    pub fn not_configured_v1() -> Self {
        Self {
            schema: CONTROL_CONTRACT_SCHEMA_V1.to_owned(),
            version: CONTROL_CONTRACT_VERSION_V1,
            provider_id: None,
            provider_kind: None,
            state: "not_configured".to_owned(),
            base_url: None,
            api_key_env: None,
            default_model: None,
            credential_present: false,
            model_discovery_supported: false,
            reason_code: Some("llm_provider_not_configured".to_owned()),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeInspectionSnapshotV1 {
    pub plugin: PluginRuntimeControlSnapshotV1,
    pub compute: ComputeRuntimeControlSnapshotV1,
    pub llm: LlmRuntimeControlSnapshotV1,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProductionRecoveryControlSnapshotV1 {
    pub read_only: bool,
    pub recovery: ProductionRecoveryViewV1,
}