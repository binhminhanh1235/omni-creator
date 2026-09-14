use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use omnicreator_application::{
    ApplicationControlService, ApplicationRuntimeInspectorV1, AssetLibraryVisualRequestV1,
    BindStudioPackRequestV1, ComputeRuntimeControlSnapshotV1, ControlErrorCodeV1, ControlErrorV1,
    ControlOperationV1, ControlResponseV1, ControlResultV1, CreateProjectRequestV1,
    CreatorRunControlServiceV1, CreatorRunRuntimeV1, ExternalVisualResultRequestV1,
    ExternalVoiceResultRequestV1, ManualContentImportRequestV1, ManualContentRequestV1,
    ManualScenePlanImportRequestV1, ManualScenePlanRequestV1, ManualVisualRequestV1,
    ManualVoiceRequestV1, PluginRuntimeControlSnapshotV1, ProjectIdRequestV1,
    RecoveryFileRequestV1, RecoveryVoiceBundleRequestV1, RenameProjectRequestV1,
    ReplaceVoiceTimingRequestV1, SetWorkflowAutomaticExecutionRequestV1,
    StartOrResumeCreatorRequestV1, CONTROL_CONTRACT_SCHEMA_V1, CONTROL_CONTRACT_VERSION_V1,
};
use omnicreator_core::{
    initial_studio_pack_catalog_v1, run_creator_content_stage_v1, run_creator_scene_stage_v1,
    Artifact, ArtifactStore, CreatorContentSceneOptionsV1, CreatorContentSceneOutcomeV1,
    CreatorContentV1, CreatorInputV1, CreatorVisualPlanV1, EffectiveStudioPackV1, LlmGatewayClient,
    LlmGatewayConfig, PortableStudioPackCatalogV1, Project, StateStore, Workspace,
    WorkspaceSession,
};
use rmcp::{
    handler::server::wrapper::Parameters, model::CallToolResult, schemars::JsonSchema, tool,
    tool_router, transport::stdio, ErrorData as McpError, ServiceExt,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

const MCP_ERROR_ENVELOPE_SCHEMA_V1: &str = "omnicreator.mcp-error";
const MCP_ERROR_ENVELOPE_VERSION_V1: u32 = 1;

#[derive(Debug, Clone)]
struct McpServerConfigV1 {
    data_root: PathBuf,
    read_only: bool,
    device_id: String,
    llmgateway_config: Option<PathBuf>,
    studio_pack_catalog: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectIdParamsV1 {
    project_id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectCreateParamsV1 {
    title: String,
    #[serde(default)]
    studio_pack_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ProjectUpdateActionV1 {
    Rename,
    BindStudioPack,
    ClearStudioPack,
    Delete,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectUpdateParamsV1 {
    action: ProjectUpdateActionV1,
    project_id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    studio_pack_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WorkflowAutoParamsV1 {
    project_id: String,
    step: String,
    enabled: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum CreatorInputKindV1 {
    Topic,
    Script,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreatorInputParamsV1 {
    kind: CreatorInputKindV1,
    text: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreatorStartParamsV1 {
    project_id: String,
    #[serde(default)]
    input: Option<CreatorInputParamsV1>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReviewParamsV1 {
    #[serde(default)]
    project_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ContentActionV1 {
    Provide,
    Import,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ContentControlParamsV1 {
    action: ContentActionV1,
    payload: Value,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum SceneActionV1 {
    Editor,
    Provide,
    Import,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SceneControlParamsV1 {
    action: SceneActionV1,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum VisualActionV1 {
    Status,
    ProvideManual,
    ChooseAsset,
    SelectStock,
    ApproveGenerated,
    PrepareExternal,
    ProvideExternal,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct VisualControlParamsV1 {
    action: VisualActionV1,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    scene_id: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct StockSelectionPayloadV1 {
    plan: CreatorVisualPlanV1,
    scene_id: String,
    candidate_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneratedApprovalPayloadV1 {
    plan: CreatorVisualPlanV1,
    scene_id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum VoiceActionV1 {
    Status,
    ProvideManual,
    ReplaceTiming,
    PrepareExternal,
    ProvideExternal,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct VoiceControlParamsV1 {
    action: VoiceActionV1,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    segment_id: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ProductionActionV1 {
    Status,
    Recovery,
    Assemble,
    RebuildExport,
    RepairVisual,
    RepairAudio,
    RepairTiming,
    RepairVoice,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProductionControlParamsV1 {
    action: ProductionActionV1,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum RuntimeInspectionKindV1 {
    All,
    Plugins,
    Compute,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RuntimeStatusParamsV1 {
    #[serde(default)]
    kind: Option<RuntimeInspectionKindV1>,
}

#[derive(Clone)]
struct OmniCreatorMcpServerV1 {
    config: McpServerConfigV1,
}

impl OmniCreatorMcpServerV1 {
    fn new(config: McpServerConfigV1) -> Self {
        Self { config }
    }

    fn with_service_v1(
        &self,
        operation: impl for<'service> FnOnce(
            &mut ApplicationControlService<'service>,
        ) -> ControlResultV1<Value>,
    ) -> ControlResultV1<Value> {
        if self.config.read_only {
            let workspace =
                Workspace::inspect(&self.config.data_root).map_err(ControlErrorV1::from)?;
            let mut service = ApplicationControlService::for_read_only(&workspace)?;
            operation(&mut service)
        } else {
            let workspace =
                Workspace::open(&self.config.data_root).map_err(ControlErrorV1::from)?;
            let session = WorkspaceSession::acquire(workspace, &self.config.device_id)
                .map_err(ControlErrorV1::from)?;
            let mut service = ApplicationControlService::for_writer(&session)?;
            operation(&mut service)
        }
    }

    fn render_v1<T: Serialize>(
        &self,
        result: ControlResultV1<T>,
    ) -> Result<CallToolResult, McpError> {
        match result {
            Ok(value) => {
                let mut value = serde_json::to_value(value)
                    .map_err(|error| McpError::internal_error(error.to_string(), None))?;
                sanitize_value_v1(&mut value, &self.config.data_root);
                Ok(CallToolResult::structured(value))
            }
            Err(error) => {
                let error = sanitize_error_v1(error, &self.config.data_root);
                Ok(CallToolResult::structured_error(json!({
                    "schema": MCP_ERROR_ENVELOPE_SCHEMA_V1,
                    "version": MCP_ERROR_ENVELOPE_VERSION_V1,
                    "error": error
                })))
            }
        }
    }

    fn load_studio_pack_catalog_v1(&self) -> ControlResultV1<PortableStudioPackCatalogV1> {
        match self.config.studio_pack_catalog.as_deref() {
            Some(path) => {
                let raw = fs::read_to_string(path).map_err(|error| {
                    ControlErrorV1::new(
                        ControlErrorCodeV1::CapabilityUnavailable,
                        format!("unable to read Studio Pack catalog: {error}"),
                    )
                })?;
                PortableStudioPackCatalogV1::from_json_v1(&raw).map_err(ControlErrorV1::from)
            }
            None => initial_studio_pack_catalog_v1().map_err(ControlErrorV1::from),
        }
    }

    fn creator_runtime_v1(&self) -> ControlResultV1<McpCreatorRunRuntimeV1> {
        McpCreatorRunRuntimeV1::new(
            self.load_studio_pack_catalog_v1()?,
            self.config.llmgateway_config.as_deref(),
        )
    }

    fn start_or_resume_v1(&self, request: StartOrResumeCreatorRequestV1) -> ControlResultV1<Value> {
        let mut runtime = self.creator_runtime_v1()?;
        if self.config.read_only {
            let workspace =
                Workspace::inspect(&self.config.data_root).map_err(ControlErrorV1::from)?;
            let mut control = CreatorRunControlServiceV1::for_read_only(&workspace)?;
            to_value_v1(control.start_or_resume_v1(&request, &mut runtime)?)
        } else {
            let workspace =
                Workspace::open(&self.config.data_root).map_err(ControlErrorV1::from)?;
            let session = WorkspaceSession::acquire(workspace, &self.config.device_id)
                .map_err(ControlErrorV1::from)?;
            let mut control = CreatorRunControlServiceV1::for_writer(&session)?;
            to_value_v1(control.start_or_resume_v1(&request, &mut runtime)?)
        }
    }
}

#[tool_router(server_handler)]
impl OmniCreatorMcpServerV1 {
    #[tool(
        description = "Inspect the active OmniCreator Data Root and writer/read-only access state."
    )]
    async fn workspace_status(&self) -> Result<CallToolResult, McpError> {
        self.render_v1(self.with_service_v1(|service| to_value_v1(service.workspace_status_v1()?)))
    }

    #[tool(description = "List canonical OmniCreator projects visible in the active Data Root.")]
    async fn projects_list(&self) -> Result<CallToolResult, McpError> {
        self.render_v1(self.with_service_v1(|service| to_value_v1(service.list_projects_v1()?)))
    }

    #[tool(description = "Get the canonical project/workflow/review projection for one project.")]
    async fn project_get(
        &self,
        Parameters(params): Parameters<ProjectIdParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        self.render_v1(self.with_service_v1(|service| {
            to_value_v1(service.project_status_v1(&ProjectIdRequestV1 {
                project_id: params.project_id,
            })?)
        }))
    }

    #[tool(
        description = "Create a project. Supply studio_pack_id to materialize the canonical creator workflow immediately."
    )]
    async fn project_create(
        &self,
        Parameters(params): Parameters<ProjectCreateParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        let catalog = if params.studio_pack_id.is_some() {
            Some(self.load_studio_pack_catalog_v1())
        } else {
            None
        };
        self.render_v1(match catalog {
            Some(Err(error)) => Err(error),
            Some(Ok(catalog)) => self.with_service_v1(|service| {
                let studio_pack_id = params
                    .studio_pack_id
                    .as_deref()
                    .expect("studio_pack_id presence checked above");
                let pack = catalog
                    .resolve_v1(studio_pack_id)
                    .map_err(ControlErrorV1::from)?;
                to_value_v1(service.create_creator_project_v1(&params.title, &pack)?)
            }),
            None => self.with_service_v1(|service| {
                to_value_v1(service.create_project_v1(&CreateProjectRequestV1 {
                    title: params.title,
                })?)
            }),
        })
    }

    #[tool(
        description = "Rename, bind/clear Studio Pack, or delete a canonical project through the shared application service."
    )]
    async fn project_update(
        &self,
        Parameters(params): Parameters<ProjectUpdateParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        self.render_v1(self.with_service_v1(|service| match params.action {
            ProjectUpdateActionV1::Rename => {
                let title = required_option_v1(params.title, "title")?;
                to_value_v1(service.rename_project_v1(&RenameProjectRequestV1 {
                    project_id: params.project_id,
                    title,
                })?)
            }
            ProjectUpdateActionV1::BindStudioPack => {
                let studio_pack_id = required_option_v1(params.studio_pack_id, "studio_pack_id")?;
                to_value_v1(service.bind_studio_pack_v1(&BindStudioPackRequestV1 {
                    project_id: params.project_id,
                    studio_pack_id: Some(studio_pack_id),
                })?)
            }
            ProjectUpdateActionV1::ClearStudioPack => {
                to_value_v1(service.bind_studio_pack_v1(&BindStudioPackRequestV1 {
                    project_id: params.project_id,
                    studio_pack_id: None,
                })?)
            }
            ProjectUpdateActionV1::Delete => {
                to_value_v1(service.delete_project_v1(&ProjectIdRequestV1 {
                    project_id: params.project_id,
                })?)
            }
        }))
    }

    #[tool(description = "Inspect per-step automatic execution policy for a creator project.")]
    async fn workflow_status(
        &self,
        Parameters(params): Parameters<ProjectIdParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        self.render_v1(self.with_service_v1(|service| {
            to_value_v1(service.workflow_execution_policies_v1(&ProjectIdRequestV1 {
                project_id: params.project_id,
            })?)
        }))
    }

    #[tool(
        description = "Turn one canonical workflow stage automatic execution ON or OFF without changing its completion state."
    )]
    async fn workflow_set_step_auto(
        &self,
        Parameters(params): Parameters<WorkflowAutoParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        self.render_v1(self.with_service_v1(|service| {
            let snapshot = service.project_status_v1(&ProjectIdRequestV1 {
                project_id: params.project_id.clone(),
            })?;
            let step_id = snapshot
                .data
                .steps
                .iter()
                .find(|step| step.step == params.step)
                .map(|step| step.step_id.clone())
                .ok_or_else(|| {
                    ControlErrorV1::new(
                        ControlErrorCodeV1::NotFound,
                        format!(
                            "workflow step {} was not found in project {}",
                            params.step, params.project_id
                        ),
                    )
                })?;
            to_value_v1(service.set_workflow_automatic_execution_v1(
                &SetWorkflowAutomaticExecutionRequestV1 {
                    step_id,
                    enabled: params.enabled,
                },
            )?)
        }))
    }

    #[tool(description = "Inspect the durable creator Start/Resume coordinator for one project.")]
    async fn creator_state(
        &self,
        Parameters(params): Parameters<ProjectIdParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        self.render_v1(self.with_service_v1(|service| {
            to_value_v1(service.creator_run_state_v1(&ProjectIdRequestV1 {
                project_id: params.project_id,
            })?)
        }))
    }

    #[tool(
        description = "Start or resume canonical creator execution. Automatic stages run only when their shared policy and local runtime capability allow it."
    )]
    async fn creator_start_or_resume(
        &self,
        Parameters(params): Parameters<CreatorStartParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        let input = params.input.map(|input| match input.kind {
            CreatorInputKindV1::Topic => CreatorInputV1::topic(input.text),
            CreatorInputKindV1::Script => CreatorInputV1::script(input.text),
        });
        self.render_v1(self.start_or_resume_v1(StartOrResumeCreatorRequestV1 {
            project_id: params.project_id,
            input,
        }))
    }

    #[tool(description = "Inspect the Review Center globally or for a single project.")]
    async fn review_list(
        &self,
        Parameters(params): Parameters<ReviewParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        self.render_v1(self.with_service_v1(|service| {
            if let Some(project_id) = params.project_id {
                let snapshot = service.project_status_v1(&ProjectIdRequestV1 { project_id })?;
                to_value_v1(ControlResponseV1::new(
                    ControlOperationV1::ReviewCenter,
                    snapshot.data.review_center,
                ))
            } else {
                to_value_v1(service.review_center_v1()?)
            }
        }))
    }

    #[tool(
        description = "Provide or import canonical creator Content. payload must match the application-control request for the selected action."
    )]
    async fn content_control(
        &self,
        Parameters(params): Parameters<ContentControlParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        self.render_v1(self.with_service_v1(|service| match params.action {
            ContentActionV1::Provide => {
                let request: ManualContentRequestV1 = parse_payload_v1(params.payload)?;
                to_value_v1(service.provide_manual_content_v1(&request)?)
            }
            ContentActionV1::Import => {
                let request: ManualContentImportRequestV1 = parse_payload_v1(params.payload)?;
                to_value_v1(service.import_manual_content_v1(&request)?)
            }
        }))
    }

    #[tool(
        description = "Open the ScenePlan editor projection, provide a canonical manual ScenePlan, or import one from an approved file path."
    )]
    async fn scene_plan_control(
        &self,
        Parameters(params): Parameters<SceneControlParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        self.render_v1(self.with_service_v1(|service| match params.action {
            SceneActionV1::Editor => {
                let project_id = required_option_v1(params.project_id, "project_id")?;
                to_value_v1(service.scene_plan_editor_v1(&ProjectIdRequestV1 { project_id })?)
            }
            SceneActionV1::Provide => {
                let request: ManualScenePlanRequestV1 =
                    parse_payload_v1(required_option_v1(params.payload, "payload")?)?;
                to_value_v1(service.provide_manual_scene_plan_v1(&request)?)
            }
            SceneActionV1::Import => {
                let request: ManualScenePlanImportRequestV1 =
                    parse_payload_v1(required_option_v1(params.payload, "payload")?)?;
                to_value_v1(service.import_manual_scene_plan_v1(&request)?)
            }
        }))
    }

    #[tool(
        description = "Inspect or satisfy creator visual work through canonical manual, asset-library, stock, generated approval, or external handoff operations."
    )]
    async fn visual_control(
        &self,
        Parameters(params): Parameters<VisualControlParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        self.render_v1(self.with_service_v1(|service| match params.action {
            VisualActionV1::Status => {
                let project_id = required_option_v1(params.project_id, "project_id")?;
                to_value_v1(service.visual_states_v1(&ProjectIdRequestV1 { project_id })?)
            }
            VisualActionV1::ProvideManual => {
                let request: ManualVisualRequestV1 =
                    parse_payload_v1(required_option_v1(params.payload, "payload")?)?;
                to_value_v1(service.provide_manual_visual_v1(&request)?)
            }
            VisualActionV1::ChooseAsset => {
                let request: AssetLibraryVisualRequestV1 =
                    parse_payload_v1(required_option_v1(params.payload, "payload")?)?;
                to_value_v1(service.choose_asset_library_visual_v1(&request)?)
            }
            VisualActionV1::SelectStock => {
                let request: StockSelectionPayloadV1 =
                    parse_payload_v1(required_option_v1(params.payload, "payload")?)?;
                to_value_v1(service.select_stock_candidate_v1(
                    request.plan,
                    &request.scene_id,
                    &request.candidate_id,
                )?)
            }
            VisualActionV1::ApproveGenerated => {
                let request: GeneratedApprovalPayloadV1 =
                    parse_payload_v1(required_option_v1(params.payload, "payload")?)?;
                to_value_v1(service.approve_generated_visual_v1(request.plan, &request.scene_id)?)
            }
            VisualActionV1::PrepareExternal => {
                let project_id = required_option_v1(params.project_id, "project_id")?;
                let scene_id = required_option_v1(params.scene_id, "scene_id")?;
                to_value_v1(service.prepare_external_visual_v1(&project_id, &scene_id)?)
            }
            VisualActionV1::ProvideExternal => {
                let request: ExternalVisualResultRequestV1 =
                    parse_payload_v1(required_option_v1(params.payload, "payload")?)?;
                to_value_v1(service.provide_external_visual_v1(&request)?)
            }
        }))
    }

    #[tool(
        description = "Inspect or satisfy creator voice/audio/timing work through canonical manual or external handoff operations."
    )]
    async fn voice_control(
        &self,
        Parameters(params): Parameters<VoiceControlParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        self.render_v1(self.with_service_v1(|service| match params.action {
            VoiceActionV1::Status => {
                let project_id = required_option_v1(params.project_id, "project_id")?;
                to_value_v1(service.voice_states_v1(&ProjectIdRequestV1 { project_id })?)
            }
            VoiceActionV1::ProvideManual => {
                let request: ManualVoiceRequestV1 =
                    parse_payload_v1(required_option_v1(params.payload, "payload")?)?;
                to_value_v1(service.provide_manual_voice_v1(&request)?)
            }
            VoiceActionV1::ReplaceTiming => {
                let request: ReplaceVoiceTimingRequestV1 =
                    parse_payload_v1(required_option_v1(params.payload, "payload")?)?;
                to_value_v1(service.replace_voice_timing_v1(&request)?)
            }
            VoiceActionV1::PrepareExternal => {
                let project_id = required_option_v1(params.project_id, "project_id")?;
                let segment_id = required_option_v1(params.segment_id, "segment_id")?;
                to_value_v1(service.prepare_external_voice_v1(&project_id, &segment_id)?)
            }
            VoiceActionV1::ProvideExternal => {
                let request: ExternalVoiceResultRequestV1 =
                    parse_payload_v1(required_option_v1(params.payload, "payload")?)?;
                to_value_v1(service.provide_external_voice_v1(&request)?)
            }
        }))
    }

    #[tool(
        description = "Inspect, assemble, recover, or export the canonical ProductionPack and Resolve-ready interchange artifacts."
    )]
    async fn production_control(
        &self,
        Parameters(params): Parameters<ProductionControlParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        self.render_v1(self.with_service_v1(|service| match params.action {
            ProductionActionV1::Status => {
                let project_id = required_option_v1(params.project_id, "project_id")?;
                to_value_v1(service.latest_production_pack_v1(&ProjectIdRequestV1 { project_id })?)
            }
            ProductionActionV1::Recovery => {
                let project_id = required_option_v1(params.project_id, "project_id")?;
                to_value_v1(service.production_recovery_v1(&ProjectIdRequestV1 { project_id })?)
            }
            ProductionActionV1::Assemble => {
                let project_id = required_option_v1(params.project_id, "project_id")?;
                to_value_v1(
                    service.assemble_production_pack_v1(&ProjectIdRequestV1 { project_id })?,
                )
            }
            ProductionActionV1::RebuildExport => {
                let project_id = required_option_v1(params.project_id, "project_id")?;
                to_value_v1(
                    service.rebuild_and_export_production_v1(&ProjectIdRequestV1 { project_id })?,
                )
            }
            ProductionActionV1::RepairVisual => {
                let request: RecoveryFileRequestV1 =
                    parse_payload_v1(required_option_v1(params.payload, "payload")?)?;
                to_value_v1(service.repair_production_visual_v1(&request)?)
            }
            ProductionActionV1::RepairAudio => {
                let request: RecoveryFileRequestV1 =
                    parse_payload_v1(required_option_v1(params.payload, "payload")?)?;
                to_value_v1(service.repair_production_audio_v1(&request)?)
            }
            ProductionActionV1::RepairTiming => {
                let request: RecoveryFileRequestV1 =
                    parse_payload_v1(required_option_v1(params.payload, "payload")?)?;
                to_value_v1(service.repair_production_timing_v1(&request)?)
            }
            ProductionActionV1::RepairVoice => {
                let request: RecoveryVoiceBundleRequestV1 =
                    parse_payload_v1(required_option_v1(params.payload, "payload")?)?;
                to_value_v1(service.repair_production_voice_bundle_v1(&request)?)
            }
        }))
    }

    #[tool(
        description = "Inspect sanitized plugin and/or compute runtime capability state visible to this local MCP process."
    )]
    async fn runtime_status(
        &self,
        Parameters(params): Parameters<RuntimeStatusParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        self.render_v1(self.with_service_v1(|service| {
            let inspector = McpRuntimeInspectorV1;
            match params.kind.unwrap_or(RuntimeInspectionKindV1::All) {
                RuntimeInspectionKindV1::All => {
                    to_value_v1(service.runtime_inspection_v1(&inspector)?)
                }
                RuntimeInspectionKindV1::Plugins => {
                    to_value_v1(service.plugin_runtime_inspection_v1(&inspector)?)
                }
                RuntimeInspectionKindV1::Compute => {
                    to_value_v1(service.compute_runtime_inspection_v1(&inspector)?)
                }
            }
        }))
    }
}

struct McpCreatorRunRuntimeV1 {
    catalog: PortableStudioPackCatalogV1,
    llm: Option<LlmGatewayClient>,
}

impl McpCreatorRunRuntimeV1 {
    fn new(
        catalog: PortableStudioPackCatalogV1,
        llmgateway_config: Option<&Path>,
    ) -> ControlResultV1<Self> {
        let llm = match llmgateway_config {
            Some(path) => Some(
                LlmGatewayClient::new(LlmGatewayConfig::load(path).map_err(ControlErrorV1::from)?)
                    .map_err(ControlErrorV1::from)?,
            ),
            None => None,
        };
        Ok(Self { catalog, llm })
    }

    fn llm_v1(&self) -> ControlResultV1<&LlmGatewayClient> {
        self.llm.as_ref().ok_or_else(|| {
            ControlErrorV1::new(
                ControlErrorCodeV1::ProviderUnavailable,
                "LLMGateway is not configured for this MCP process; pass --llmgateway-config <path> and provide its credential through the referenced environment variable",
            )
        })
    }
}

impl CreatorRunRuntimeV1 for McpCreatorRunRuntimeV1 {
    fn resolve_studio_pack_v1(
        &mut self,
        studio_pack_id: &str,
    ) -> ControlResultV1<EffectiveStudioPackV1> {
        self.catalog
            .resolve_v1(studio_pack_id)
            .map_err(ControlErrorV1::from)
    }

    fn run_content_v1(
        &mut self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        project_id: &str,
        input: &CreatorInputV1,
    ) -> ControlResultV1<()> {
        run_creator_content_stage_v1(
            state_store,
            artifact_store,
            self.llm_v1()?,
            project_id,
            input,
        )
        .map(|_| ())
        .map_err(ControlErrorV1::from)
    }

    fn run_scene_v1(
        &mut self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        project_id: &str,
        content: &CreatorContentV1,
        content_artifact: &Artifact,
        options: &CreatorContentSceneOptionsV1,
    ) -> ControlResultV1<()> {
        run_creator_scene_stage_v1(
            state_store,
            artifact_store,
            self.llm_v1()?,
            project_id,
            content,
            content_artifact,
            options,
        )
        .map(|_| ())
        .map_err(ControlErrorV1::from)
    }

    fn run_visual_v1(
        &mut self,
        _state_store: &mut StateStore,
        _artifact_store: &ArtifactStore,
        _project: &Project,
        _studio_pack: &EffectiveStudioPackV1,
        _creator: &CreatorContentSceneOutcomeV1,
    ) -> ControlResultV1<bool> {
        Err(ControlErrorV1::new(
            ControlErrorCodeV1::CapabilityUnavailable,
            "automatic visual runtime is not configured in the MCP adapter; use canonical manual/external visual takeover or Desktop for machine-local plugin execution",
        ))
    }

    fn run_voice_v1(
        &mut self,
        _state_store: &mut StateStore,
        _artifact_store: &ArtifactStore,
        _studio_pack: &EffectiveStudioPackV1,
        _content: &CreatorContentV1,
    ) -> ControlResultV1<bool> {
        Err(ControlErrorV1::new(
            ControlErrorCodeV1::CapabilityUnavailable,
            "automatic voice/compute runtime is not configured in the MCP adapter; use canonical manual/external voice takeover or Desktop for machine-local ComputeProvider execution",
        ))
    }
}

struct McpRuntimeInspectorV1;

impl ApplicationRuntimeInspectorV1 for McpRuntimeInspectorV1 {
    fn plugin_runtime_snapshot_v1(&self) -> ControlResultV1<PluginRuntimeControlSnapshotV1> {
        Ok(PluginRuntimeControlSnapshotV1 {
            schema: CONTROL_CONTRACT_SCHEMA_V1.to_owned(),
            version: CONTROL_CONTRACT_VERSION_V1,
            plugins: Vec::new(),
        })
    }

    fn compute_runtime_snapshot_v1(&self) -> ControlResultV1<ComputeRuntimeControlSnapshotV1> {
        Ok(ComputeRuntimeControlSnapshotV1 {
            schema: CONTROL_CONTRACT_SCHEMA_V1.to_owned(),
            version: CONTROL_CONTRACT_VERSION_V1,
            provider_id: "none".to_owned(),
            state: "not_configured".to_owned(),
            capabilities: Vec::new(),
            reason_code: Some("mcp_runtime_not_configured".to_owned()),
        })
    }
}

pub fn is_mcp_invocation_v1(args: &[String]) -> bool {
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--data-root" | "--device-id" | "--llmgateway-config" | "--studio-pack-catalog" => {
                index += 2;
            }
            "--read-only" | "--json" => {
                index += 1;
            }
            positional => return positional == "mcp",
        }
    }
    false
}

pub async fn serve_mcp_stdio_from_args_v1(args: Vec<String>) -> Result<(), String> {
    let config = parse_mcp_config_v1(args).map_err(|error| error.to_string())?;
    let service = OmniCreatorMcpServerV1::new(config)
        .serve(stdio())
        .await
        .map_err(|error| format!("unable to start MCP stdio service: {error}"))?;
    service
        .waiting()
        .await
        .map_err(|error| format!("MCP stdio service stopped with an error: {error}"))?;
    Ok(())
}

fn parse_mcp_config_v1(args: Vec<String>) -> ControlResultV1<McpServerConfigV1> {
    let mut data_root = None::<PathBuf>;
    let mut read_only = false;
    let mut device_id = None::<String>;
    let mut llmgateway_config = None::<PathBuf>;
    let mut studio_pack_catalog = None::<PathBuf>;
    let mut command = Vec::<String>::new();
    let mut index = 0usize;

    while index < args.len() {
        match args[index].as_str() {
            "--data-root" => {
                data_root = Some(PathBuf::from(require_next_arg_v1(
                    &args,
                    index,
                    "--data-root",
                )?));
                index += 2;
            }
            "--read-only" => {
                read_only = true;
                index += 1;
            }
            "--json" => {
                index += 1;
            }
            "--device-id" => {
                device_id = Some(require_next_arg_v1(&args, index, "--device-id")?);
                index += 2;
            }
            "--llmgateway-config" => {
                llmgateway_config = Some(PathBuf::from(require_next_arg_v1(
                    &args,
                    index,
                    "--llmgateway-config",
                )?));
                index += 2;
            }
            "--studio-pack-catalog" => {
                studio_pack_catalog = Some(PathBuf::from(require_next_arg_v1(
                    &args,
                    index,
                    "--studio-pack-catalog",
                )?));
                index += 2;
            }
            _ => {
                command.push(args[index].clone());
                index += 1;
            }
        }
    }

    if command.len() != 2 || command[0] != "mcp" || command[1] != "serve" {
        return Err(ControlErrorV1::new(
            ControlErrorCodeV1::InvalidInput,
            "MCP mode requires exactly: mcp serve",
        ));
    }

    Ok(McpServerConfigV1 {
        data_root: data_root.ok_or_else(|| {
            ControlErrorV1::new(
                ControlErrorCodeV1::InvalidInput,
                "--data-root is required for MCP mode",
            )
        })?,
        read_only,
        device_id: device_id.unwrap_or_else(default_mcp_device_id_v1),
        llmgateway_config,
        studio_pack_catalog,
    })
}

fn require_next_arg_v1(args: &[String], index: usize, flag: &str) -> ControlResultV1<String> {
    args.get(index + 1).cloned().ok_or_else(|| {
        ControlErrorV1::new(
            ControlErrorCodeV1::InvalidInput,
            format!("{flag} requires a value"),
        )
    })
}

fn default_mcp_device_id_v1() -> String {
    for key in ["OMNICREATOR_DEVICE_ID", "HOSTNAME", "COMPUTERNAME"] {
        if let Ok(value) = env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return format!("mcp-{value}");
            }
        }
    }
    "omnicreator-mcp-local".to_owned()
}

fn parse_payload_v1<T: DeserializeOwned>(payload: Value) -> ControlResultV1<T> {
    serde_json::from_value(payload).map_err(|error| {
        ControlErrorV1::new(
            ControlErrorCodeV1::InvalidInput,
            format!("invalid structured payload: {error}"),
        )
    })
}

fn required_option_v1<T>(value: Option<T>, field: &str) -> ControlResultV1<T> {
    value.ok_or_else(|| {
        ControlErrorV1::new(
            ControlErrorCodeV1::InvalidInput,
            format!("{field} is required for this action"),
        )
    })
}

fn to_value_v1<T: Serialize>(value: T) -> ControlResultV1<Value> {
    serde_json::to_value(value).map_err(|error| {
        ControlErrorV1::new(
            ControlErrorCodeV1::Internal,
            format!("unable to serialize MCP result: {error}"),
        )
    })
}

fn sanitize_value_v1(value: &mut Value, data_root: &Path) {
    match value {
        Value::String(text) => {
            *text = sanitize_text_v1(text, data_root);
        }
        Value::Array(items) => {
            for item in items {
                sanitize_value_v1(item, data_root);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                sanitize_value_v1(item, data_root);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn sanitize_text_v1(text: &str, data_root: &Path) -> String {
    let mut sanitized = text.to_owned();
    let mut replacements = BTreeMap::<String, &str>::new();
    replacements.insert(data_root.to_string_lossy().to_string(), "<data-root>");
    if let Ok(canonical) = fs::canonicalize(data_root) {
        replacements.insert(canonical.to_string_lossy().to_string(), "<data-root>");
    }
    for (needle, replacement) in replacements {
        if !needle.is_empty() {
            sanitized = sanitized.replace(&needle, replacement);
        }
    }
    sanitized.replace("Bearer ", "Bearer <redacted>")
}

fn sanitize_error_v1(mut error: ControlErrorV1, data_root: &Path) -> ControlErrorV1 {
    error.message = sanitize_text_v1(&error.message, data_root);
    error
}
