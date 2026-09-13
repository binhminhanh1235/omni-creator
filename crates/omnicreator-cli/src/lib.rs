use std::{
    collections::BTreeMap,
    env, fs,
    io::Read,
    path::{Path, PathBuf},
};

use omnicreator_application::{
    ApplicationControlService, ApplicationRuntimeInspectorV1, AssetLibraryVisualRequestV1,
    BindStudioPackRequestV1, ComputeRuntimeControlSnapshotV1, ControlErrorCodeV1, ControlErrorV1,
    ControlOperationV1, ControlResponseV1, ControlResultV1, CreatorRunControlServiceV1,
    CreatorRunRuntimeV1, ExternalVisualResultRequestV1, ExternalVoiceResultRequestV1,
    ManualContentImportRequestV1, ManualContentRequestV1, ManualScenePlanImportRequestV1,
    ManualScenePlanRequestV1, ManualVisualRequestV1, ManualVoiceRequestV1,
    PluginRuntimeControlSnapshotV1, ProjectIdRequestV1, RecoveryFileRequestV1,
    RecoveryVoiceBundleRequestV1, RenameProjectRequestV1, ReplaceVoiceTimingRequestV1,
    SetWorkflowAutomaticExecutionRequestV1, StartOrResumeCreatorRequestV1,
    CONTROL_CONTRACT_SCHEMA_V1, CONTROL_CONTRACT_VERSION_V1,
};
use omnicreator_core::{
    initial_studio_pack_catalog_v1, run_creator_content_stage_v1, run_creator_scene_stage_v1,
    Artifact, ArtifactStore, CreatorContentSceneOptionsV1, CreatorContentSceneOutcomeV1,
    CreatorContentV1, CreatorInputV1, CreatorVisualPlanV1, EffectiveStudioPackV1, LlmGatewayClient,
    LlmGatewayConfig, PortableStudioPackCatalogV1, Project, StateStore, Workspace,
    WorkspaceSession,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

pub const CLI_RESPONSE_SCHEMA_V1: &str = "omnicreator.cli-response";
pub const CLI_RESPONSE_VERSION_V1: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliRunResultV1 {
    pub exit_code: i32,
    pub output: String,
}

#[derive(Debug, Serialize)]
struct CliEnvelopeV1 {
    schema: &'static str,
    version: u32,
    ok: bool,
    operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ControlErrorV1>,
}

#[derive(Debug, Clone)]
struct GlobalOptionsV1 {
    data_root: PathBuf,
    read_only: bool,
    json: bool,
    device_id: String,
    llmgateway_config: Option<PathBuf>,
    studio_pack_catalog: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct CliInvocationV1 {
    global: GlobalOptionsV1,
    resource: String,
    verb: String,
    command_args: Vec<String>,
}

impl CliInvocationV1 {
    fn operation(&self) -> String {
        format!("{}.{}", self.resource, self.verb)
    }
}

#[derive(Debug, Clone)]
struct CommandArgsV1 {
    items: Vec<String>,
}

impl CommandArgsV1 {
    fn new(items: Vec<String>) -> Self {
        Self { items }
    }

    fn take_flag(&mut self, flag: &str) -> bool {
        if let Some(index) = self.items.iter().position(|item| item == flag) {
            self.items.remove(index);
            true
        } else {
            false
        }
    }

    fn take_value(&mut self, flag: &str) -> ControlResultV1<Option<String>> {
        let Some(index) = self.items.iter().position(|item| item == flag) else {
            return Ok(None);
        };
        if index + 1 >= self.items.len() {
            return Err(invalid_input_v1(format!("{flag} requires a value")));
        }
        self.items.remove(index);
        Ok(Some(self.items.remove(index)))
    }

    fn require_value(&mut self, flag: &str) -> ControlResultV1<String> {
        self.take_value(flag)?
            .ok_or_else(|| invalid_input_v1(format!("missing required {flag}")))
    }

    fn finish(&self) -> ControlResultV1<()> {
        if self.items.is_empty() {
            Ok(())
        } else {
            Err(invalid_input_v1(format!(
                "unexpected arguments: {}",
                self.items.join(" ")
            )))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StockSelectionCliRequestV1 {
    plan: CreatorVisualPlanV1,
    scene_id: String,
    candidate_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneratedApprovalCliRequestV1 {
    plan: CreatorVisualPlanV1,
    scene_id: String,
}

pub fn run_cli_v1(args: Vec<String>, stdin: &mut dyn Read) -> CliRunResultV1 {
    let json_hint = args.iter().any(|item| item == "--json");
    match parse_invocation_v1(args) {
        Ok(invocation) => {
            let operation = invocation.operation();
            let data_root = invocation.global.data_root.clone();
            let json = invocation.global.json;
            match execute_invocation_v1(&invocation, stdin) {
                Ok(data) => render_v1(
                    CliEnvelopeV1 {
                        schema: CLI_RESPONSE_SCHEMA_V1,
                        version: CLI_RESPONSE_VERSION_V1,
                        ok: true,
                        operation,
                        data: Some(data),
                        error: None,
                    },
                    json,
                    0,
                ),
                Err(error) => {
                    let error = sanitize_error_v1(error, &data_root);
                    let exit_code = exit_code_v1(error.code);
                    render_v1(
                        CliEnvelopeV1 {
                            schema: CLI_RESPONSE_SCHEMA_V1,
                            version: CLI_RESPONSE_VERSION_V1,
                            ok: false,
                            operation,
                            data: None,
                            error: Some(error),
                        },
                        json,
                        exit_code,
                    )
                }
            }
        }
        Err(error) => render_v1(
            CliEnvelopeV1 {
                schema: CLI_RESPONSE_SCHEMA_V1,
                version: CLI_RESPONSE_VERSION_V1,
                ok: false,
                operation: "cli.parse".to_owned(),
                data: None,
                error: Some(error.clone()),
            },
            json_hint,
            exit_code_v1(error.code),
        ),
    }
}

fn render_v1(envelope: CliEnvelopeV1, json: bool, exit_code: i32) -> CliRunResultV1 {
    let output = if json {
        serde_json::to_string(&envelope)
    } else {
        serde_json::to_string_pretty(&envelope)
    }
    .unwrap_or_else(|error| {
        format!(
            "{{\"schema\":\"{CLI_RESPONSE_SCHEMA_V1}\",\"version\":{CLI_RESPONSE_VERSION_V1},\"ok\":false,\"operation\":\"cli.serialize\",\"error\":{{\"code\":\"internal\",\"message\":\"serialization failed: {error}\"}}}}"
        )
    });
    CliRunResultV1 { exit_code, output }
}

fn parse_invocation_v1(args: Vec<String>) -> ControlResultV1<CliInvocationV1> {
    let mut data_root = None::<PathBuf>;
    let mut read_only = false;
    let mut json = false;
    let mut device_id = None::<String>;
    let mut llmgateway_config = None::<PathBuf>;
    let mut studio_pack_catalog = None::<PathBuf>;
    let mut command = Vec::<String>::new();
    let mut index = 0usize;

    while index < args.len() {
        match args[index].as_str() {
            "--data-root" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| invalid_input_v1("--data-root requires a value"))?;
                data_root = Some(PathBuf::from(value));
                index += 2;
            }
            "--read-only" => {
                read_only = true;
                index += 1;
            }
            "--json" => {
                json = true;
                index += 1;
            }
            "--device-id" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| invalid_input_v1("--device-id requires a value"))?;
                device_id = Some(value.clone());
                index += 2;
            }
            "--llmgateway-config" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| invalid_input_v1("--llmgateway-config requires a value"))?;
                llmgateway_config = Some(PathBuf::from(value));
                index += 2;
            }
            "--studio-pack-catalog" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| invalid_input_v1("--studio-pack-catalog requires a value"))?;
                studio_pack_catalog = Some(PathBuf::from(value));
                index += 2;
            }
            _ => {
                command.push(args[index].clone());
                index += 1;
            }
        }
    }

    if command.len() < 2 {
        return Err(invalid_input_v1(
            "expected <resource> <verb>; for example: workspace status",
        ));
    }
    let data_root = data_root.ok_or_else(|| invalid_input_v1("--data-root is required"))?;
    let resource = command.remove(0);
    let verb = command.remove(0);
    Ok(CliInvocationV1 {
        global: GlobalOptionsV1 {
            data_root,
            read_only,
            json,
            device_id: device_id.unwrap_or_else(default_device_id_v1),
            llmgateway_config,
            studio_pack_catalog,
        },
        resource,
        verb,
        command_args: command,
    })
}

fn execute_invocation_v1(
    invocation: &CliInvocationV1,
    stdin: &mut dyn Read,
) -> ControlResultV1<Value> {
    if invocation.resource == "workspace" && invocation.verb == "init" {
        if invocation.global.read_only {
            return Err(invalid_input_v1(
                "workspace init cannot be used with --read-only",
            ));
        }
        let args = CommandArgsV1::new(invocation.command_args.clone());
        args.finish()?;
        let workspace =
            Workspace::create(&invocation.global.data_root).map_err(ControlErrorV1::from)?;
        let session = WorkspaceSession::acquire(workspace, &invocation.global.device_id)
            .map_err(ControlErrorV1::from)?;
        let service = ApplicationControlService::for_writer(&session)?;
        return value_v1(service.workspace_status_v1()?);
    }

    if invocation.resource == "creator" && matches!(invocation.verb.as_str(), "start" | "resume") {
        return execute_creator_run_v1(invocation, stdin);
    }

    if invocation.global.read_only {
        let workspace =
            Workspace::inspect(&invocation.global.data_root).map_err(ControlErrorV1::from)?;
        let mut service = ApplicationControlService::for_read_only(&workspace)?;
        execute_service_command_v1(&mut service, invocation, stdin)
    } else {
        let workspace =
            Workspace::open(&invocation.global.data_root).map_err(ControlErrorV1::from)?;
        let session = WorkspaceSession::acquire(workspace, &invocation.global.device_id)
            .map_err(ControlErrorV1::from)?;
        let mut service = ApplicationControlService::for_writer(&session)?;
        execute_service_command_v1(&mut service, invocation, stdin)
    }
}

fn execute_creator_run_v1(
    invocation: &CliInvocationV1,
    stdin: &mut dyn Read,
) -> ControlResultV1<Value> {
    let request = creator_request_v1(invocation.command_args.clone(), stdin)?;
    let catalog = load_studio_pack_catalog_v1(invocation.global.studio_pack_catalog.as_deref())?;
    let mut runtime =
        CliCreatorRunRuntimeV1::new(catalog, invocation.global.llmgateway_config.as_deref())?;

    if invocation.global.read_only {
        let workspace =
            Workspace::inspect(&invocation.global.data_root).map_err(ControlErrorV1::from)?;
        let mut control = CreatorRunControlServiceV1::for_read_only(&workspace)?;
        value_v1(control.start_or_resume_v1(&request, &mut runtime)?)
    } else {
        let workspace =
            Workspace::open(&invocation.global.data_root).map_err(ControlErrorV1::from)?;
        let session = WorkspaceSession::acquire(workspace, &invocation.global.device_id)
            .map_err(ControlErrorV1::from)?;
        let mut control = CreatorRunControlServiceV1::for_writer(&session)?;
        value_v1(control.start_or_resume_v1(&request, &mut runtime)?)
    }
}

fn creator_request_v1(
    command_args: Vec<String>,
    stdin: &mut dyn Read,
) -> ControlResultV1<StartOrResumeCreatorRequestV1> {
    let mut args = CommandArgsV1::new(command_args);
    if has_payload_mode_v1(&args) {
        return read_payload_v1(&mut args, stdin);
    }

    let project_id = args.require_value("--project")?;
    let topic = args.take_value("--topic")?;
    let script = args.take_value("--script")?;
    let topic_file = args.take_value("--topic-file")?;
    let script_file = args.take_value("--script-file")?;
    let supplied = [
        topic.is_some(),
        script.is_some(),
        topic_file.is_some(),
        script_file.is_some(),
    ]
    .into_iter()
    .filter(|value| *value)
    .count();
    if supplied > 1 {
        return Err(invalid_input_v1(
            "use only one of --topic, --script, --topic-file, or --script-file",
        ));
    }
    let input = if let Some(text) = topic {
        Some(CreatorInputV1::topic(text))
    } else if let Some(text) = script {
        Some(CreatorInputV1::script(text))
    } else if let Some(path) = topic_file {
        Some(CreatorInputV1::topic(read_text_file_v1(&path)?))
    } else if let Some(path) = script_file {
        Some(CreatorInputV1::script(read_text_file_v1(&path)?))
    } else {
        None
    };
    args.finish()?;
    Ok(StartOrResumeCreatorRequestV1 { project_id, input })
}

fn execute_service_command_v1(
    service: &mut ApplicationControlService<'_>,
    invocation: &CliInvocationV1,
    stdin: &mut dyn Read,
) -> ControlResultV1<Value> {
    let mut args = CommandArgsV1::new(invocation.command_args.clone());
    match (invocation.resource.as_str(), invocation.verb.as_str()) {
        ("workspace", "status") => {
            args.finish()?;
            value_v1(service.workspace_status_v1()?)
        }
        ("project", "list") => {
            args.finish()?;
            value_v1(service.list_projects_v1()?)
        }
        ("project", "show") | ("project", "status") => {
            let project_id = args.require_value("--project")?;
            args.finish()?;
            value_v1(service.project_status_v1(&ProjectIdRequestV1 { project_id })?)
        }
        ("project", "create") => {
            let title = args.require_value("--title")?;
            let studio_pack = args.take_value("--studio-pack")?;
            args.finish()?;
            if let Some(studio_pack) = studio_pack {
                let catalog =
                    load_studio_pack_catalog_v1(invocation.global.studio_pack_catalog.as_deref())?;
                let pack = catalog
                    .resolve_v1(&studio_pack)
                    .map_err(ControlErrorV1::from)?;
                value_v1(service.create_creator_project_v1(&title, &pack)?)
            } else {
                value_v1(service.create_project_v1(
                    &omnicreator_application::CreateProjectRequestV1 { title },
                )?)
            }
        }
        ("project", "rename") => {
            let project_id = args.require_value("--project")?;
            let title = args.require_value("--title")?;
            args.finish()?;
            value_v1(service.rename_project_v1(&RenameProjectRequestV1 { project_id, title })?)
        }
        ("project", "delete") => {
            let project_id = args.require_value("--project")?;
            args.finish()?;
            value_v1(service.delete_project_v1(&ProjectIdRequestV1 { project_id })?)
        }
        ("project", "bind-studio-pack") => {
            let project_id = args.require_value("--project")?;
            let clear = args.take_flag("--clear");
            let studio_pack_id = args.take_value("--studio-pack")?;
            if clear == studio_pack_id.is_some() {
                return Err(invalid_input_v1(
                    "use exactly one of --studio-pack <id> or --clear",
                ));
            }
            args.finish()?;
            value_v1(service.bind_studio_pack_v1(&BindStudioPackRequestV1 {
                project_id,
                studio_pack_id,
            })?)
        }
        ("workflow", "status") => {
            let project_id = args.require_value("--project")?;
            args.finish()?;
            value_v1(service.workflow_execution_policies_v1(&ProjectIdRequestV1 { project_id })?)
        }
        ("workflow", "set-auto") | ("step", "auto") => {
            let project_id = args.require_value("--project")?;
            let step_key = args.require_value("--step")?;
            let on = args.take_flag("--on");
            let off = args.take_flag("--off");
            if on == off {
                return Err(invalid_input_v1("use exactly one of --on or --off"));
            }
            args.finish()?;
            let snapshot = service.project_status_v1(&ProjectIdRequestV1 {
                project_id: project_id.clone(),
            })?;
            let step_id = snapshot
                .data
                .steps
                .iter()
                .find(|step| step.step == step_key)
                .map(|step| step.step_id.clone())
                .ok_or_else(|| {
                    ControlErrorV1::new(
                        ControlErrorCodeV1::NotFound,
                        format!("workflow step {step_key} was not found in project {project_id}"),
                    )
                })?;
            value_v1(service.set_workflow_automatic_execution_v1(
                &SetWorkflowAutomaticExecutionRequestV1 {
                    step_id,
                    enabled: on,
                },
            )?)
        }
        ("creator", "state") | ("creator", "status") => {
            let project_id = args.require_value("--project")?;
            args.finish()?;
            value_v1(service.creator_run_state_v1(&ProjectIdRequestV1 { project_id })?)
        }
        ("review", "list") | ("review", "status") => {
            let project_id = args.take_value("--project")?;
            args.finish()?;
            if let Some(project_id) = project_id {
                let snapshot =
                    service.project_status_v1(&ProjectIdRequestV1 { project_id })?;
                value_v1(ControlResponseV1::new(
                    ControlOperationV1::ReviewCenter,
                    snapshot.data.review_center,
                ))
            } else {
                value_v1(service.review_center_v1()?)
            }
        }
        ("content", "provide") => {
            let request: ManualContentRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.provide_manual_content_v1(&request)?)
        }
        ("content", "import") => {
            let request: ManualContentImportRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.import_manual_content_v1(&request)?)
        }
        ("scene", "editor") => {
            let project_id = args.require_value("--project")?;
            args.finish()?;
            value_v1(service.scene_plan_editor_v1(&ProjectIdRequestV1 { project_id })?)
        }
        ("scene", "provide") => {
            let request: ManualScenePlanRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.provide_manual_scene_plan_v1(&request)?)
        }
        ("scene", "import") => {
            let request: ManualScenePlanImportRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.import_manual_scene_plan_v1(&request)?)
        }
        ("visual", "list") | ("visual", "status") => {
            let project_id = args.require_value("--project")?;
            args.finish()?;
            value_v1(service.visual_states_v1(&ProjectIdRequestV1 { project_id })?)
        }
        ("visual", "provide") => {
            let request: ManualVisualRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.provide_manual_visual_v1(&request)?)
        }
        ("visual", "asset") => {
            let request: AssetLibraryVisualRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.choose_asset_library_visual_v1(&request)?)
        }
        ("visual", "select-stock") => {
            let request: StockSelectionCliRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.select_stock_candidate_v1(
                request.plan,
                &request.scene_id,
                &request.candidate_id,
            )?)
        }
        ("visual", "approve-generated") => {
            let request: GeneratedApprovalCliRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.approve_generated_visual_v1(request.plan, &request.scene_id)?)
        }
        ("visual", "prepare-external") => {
            let project_id = args.require_value("--project")?;
            let scene_id = args.require_value("--scene")?;
            args.finish()?;
            value_v1(service.prepare_external_visual_v1(&project_id, &scene_id)?)
        }
        ("visual", "provide-external") => {
            let request: ExternalVisualResultRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.provide_external_visual_v1(&request)?)
        }
        ("voice", "list") | ("voice", "status") => {
            let project_id = args.require_value("--project")?;
            args.finish()?;
            value_v1(service.voice_states_v1(&ProjectIdRequestV1 { project_id })?)
        }
        ("voice", "provide") => {
            let request: ManualVoiceRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.provide_manual_voice_v1(&request)?)
        }
        ("voice", "replace-timing") => {
            let request: ReplaceVoiceTimingRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.replace_voice_timing_v1(&request)?)
        }
        ("voice", "prepare-external") => {
            let project_id = args.require_value("--project")?;
            let segment_id = args.require_value("--segment")?;
            args.finish()?;
            value_v1(service.prepare_external_voice_v1(&project_id, &segment_id)?)
        }
        ("voice", "provide-external") => {
            let request: ExternalVoiceResultRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.provide_external_voice_v1(&request)?)
        }
        ("production", "status") | ("production", "latest") => {
            let project_id = args.require_value("--project")?;
            args.finish()?;
            value_v1(service.latest_production_pack_v1(&ProjectIdRequestV1 { project_id })?)
        }
        ("production", "recovery") => {
            let project_id = args.require_value("--project")?;
            args.finish()?;
            value_v1(service.production_recovery_v1(&ProjectIdRequestV1 { project_id })?)
        }
        ("production", "assemble") => {
            let project_id = args.require_value("--project")?;
            args.finish()?;
            value_v1(service.assemble_production_pack_v1(&ProjectIdRequestV1 { project_id })?)
        }
        ("production", "rebuild-export") | ("production", "export") => {
            let project_id = args.require_value("--project")?;
            args.finish()?;
            value_v1(service.rebuild_and_export_production_v1(&ProjectIdRequestV1 { project_id })?)
        }
        ("production", "repair-visual") => {
            let request: RecoveryFileRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.repair_production_visual_v1(&request)?)
        }
        ("production", "repair-audio") => {
            let request: RecoveryFileRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.repair_production_audio_v1(&request)?)
        }
        ("production", "repair-timing") => {
            let request: RecoveryFileRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.repair_production_timing_v1(&request)?)
        }
        ("production", "repair-voice") => {
            let request: RecoveryVoiceBundleRequestV1 = read_payload_v1(&mut args, stdin)?;
            value_v1(service.repair_production_voice_bundle_v1(&request)?)
        }
        ("runtime", "status") => {
            args.finish()?;
            value_v1(service.runtime_inspection_v1(&CliRuntimeInspectorV1)?)
        }
        ("runtime", "plugins") | ("plugin", "status") => {
            args.finish()?;
            value_v1(service.plugin_runtime_inspection_v1(&CliRuntimeInspectorV1)?)
        }
        ("runtime", "compute") | ("compute", "status") => {
            args.finish()?;
            value_v1(service.compute_runtime_inspection_v1(&CliRuntimeInspectorV1)?)
        }
        _ => Err(invalid_input_v1(format!(
            "unsupported command {} {}",
            invocation.resource, invocation.verb
        ))),
    }
}

struct CliCreatorRunRuntimeV1 {
    catalog: PortableStudioPackCatalogV1,
    llm: Option<LlmGatewayClient>,
}

impl CliCreatorRunRuntimeV1 {
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
                "LLMGateway is not configured for this CLI process; pass --llmgateway-config <path> and provide its credential through the referenced environment variable",
            )
        })
    }
}

impl CreatorRunRuntimeV1 for CliCreatorRunRuntimeV1 {
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
            "automatic visual runtime is not configured in the CLI adapter; use canonical manual/external visual takeover or Desktop for machine-local plugin execution",
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
            "automatic voice/compute runtime is not configured in the CLI adapter; use canonical manual/external voice takeover or Desktop for machine-local ComputeProvider execution",
        ))
    }
}

struct CliRuntimeInspectorV1;

impl ApplicationRuntimeInspectorV1 for CliRuntimeInspectorV1 {
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
            reason_code: Some("cli_runtime_not_configured".to_owned()),
        })
    }
}

fn has_payload_mode_v1(args: &CommandArgsV1) -> bool {
    args.items
        .iter()
        .any(|item| item == "--stdin" || item == "--input-file")
}

fn read_payload_v1<T: DeserializeOwned>(
    args: &mut CommandArgsV1,
    stdin: &mut dyn Read,
) -> ControlResultV1<T> {
    let use_stdin = args.take_flag("--stdin");
    let input_file = args.take_value("--input-file")?;
    if use_stdin == input_file.is_some() {
        return Err(invalid_input_v1(
            "use exactly one of --stdin or --input-file <path> for a structured payload",
        ));
    }
    let raw = if use_stdin {
        let mut raw = String::new();
        stdin.read_to_string(&mut raw).map_err(|error| {
            ControlErrorV1::new(ControlErrorCodeV1::InvalidInput, error.to_string())
        })?;
        raw
    } else {
        let path = input_file.expect("input_file presence checked above");
        read_text_file_v1(&path)?
    };
    args.finish()?;
    serde_json::from_str(&raw).map_err(|error| {
        ControlErrorV1::new(
            ControlErrorCodeV1::InvalidInput,
            format!("invalid structured JSON payload: {error}"),
        )
    })
}

fn read_text_file_v1(path: &str) -> ControlResultV1<String> {
    fs::read_to_string(path).map_err(|error| {
        ControlErrorV1::new(
            ControlErrorCodeV1::InvalidInput,
            format!("unable to read input file: {error}"),
        )
    })
}

fn load_studio_pack_catalog_v1(
    path: Option<&Path>,
) -> ControlResultV1<PortableStudioPackCatalogV1> {
    match path {
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

fn default_device_id_v1() -> String {
    for key in ["OMNICREATOR_DEVICE_ID", "HOSTNAME", "COMPUTERNAME"] {
        if let Ok(value) = env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return format!("cli-{value}");
            }
        }
    }
    "omnicreator-cli-local".to_owned()
}

fn value_v1<T: Serialize>(value: T) -> ControlResultV1<Value> {
    serde_json::to_value(value).map_err(|error| {
        ControlErrorV1::new(
            ControlErrorCodeV1::Internal,
            format!("unable to serialize CLI result: {error}"),
        )
    })
}

fn invalid_input_v1(message: impl Into<String>) -> ControlErrorV1 {
    ControlErrorV1::new(ControlErrorCodeV1::InvalidInput, message)
}

fn sanitize_error_v1(mut error: ControlErrorV1, data_root: &Path) -> ControlErrorV1 {
    let mut replacements = BTreeMap::<String, &str>::new();
    replacements.insert(data_root.to_string_lossy().to_string(), "<data-root>");
    if let Ok(canonical) = fs::canonicalize(data_root) {
        replacements.insert(canonical.to_string_lossy().to_string(), "<data-root>");
    }
    for (needle, replacement) in replacements {
        if !needle.is_empty() {
            error.message = error.message.replace(&needle, replacement);
        }
    }
    error.message = error.message.replace("Bearer ", "Bearer <redacted>");
    error
}

fn exit_code_v1(code: ControlErrorCodeV1) -> i32 {
    match code {
        ControlErrorCodeV1::InvalidInput => 2,
        ControlErrorCodeV1::NotFound => 3,
        ControlErrorCodeV1::ReadOnly => 4,
        ControlErrorCodeV1::WriterConflict => 5,
        ControlErrorCodeV1::InvalidTransition => 6,
        ControlErrorCodeV1::Blocked => 7,
        ControlErrorCodeV1::CapabilityUnavailable => 8,
        ControlErrorCodeV1::ProviderUnavailable => 9,
        ControlErrorCodeV1::ArtifactInvalid => 10,
        ControlErrorCodeV1::StaleInput => 11,
        ControlErrorCodeV1::Internal => 70,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_exit_codes_cover_every_machine_error_category() {
        let cases = [
            (ControlErrorCodeV1::InvalidInput, 2),
            (ControlErrorCodeV1::NotFound, 3),
            (ControlErrorCodeV1::ReadOnly, 4),
            (ControlErrorCodeV1::WriterConflict, 5),
            (ControlErrorCodeV1::InvalidTransition, 6),
            (ControlErrorCodeV1::Blocked, 7),
            (ControlErrorCodeV1::CapabilityUnavailable, 8),
            (ControlErrorCodeV1::ProviderUnavailable, 9),
            (ControlErrorCodeV1::ArtifactInvalid, 10),
            (ControlErrorCodeV1::StaleInput, 11),
            (ControlErrorCodeV1::Internal, 70),
        ];
        for (code, expected) in cases {
            assert_eq!(exit_code_v1(code), expected);
        }
    }
}
