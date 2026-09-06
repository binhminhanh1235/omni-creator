use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use chrono::{DateTime, Utc};
use omnicreator_core::{
    assemble_creator_production_pack_v1, build_studio_pack_ux_view_v1,
    build_studio_review_center_v1, compile_creator_workflow_plan_v1,
    default_segment_tts_compute_requirements_v1, derive_creator_run_coordinator_v1,
    dispatch_creator_voice_burst_v1, dispatch_gpu_burst_v1, execute_creator_visual_plan_v1,
    initial_studio_pack_catalog_v1, inspect_local_plugin_update_v1, install_local_plugin_folder_v1,
    load_latest_creator_content_scene_v1, load_latest_creator_production_pack_v1,
    load_plugin_settings_ui, materialize_creator_workflow_plan_v1, plan_creator_visuals_v1,
    plan_creator_voice_orchestration_v1, preview_plugin_capability_impact_v1,
    project_board_projection_v1, reconcile_remote_session_v1, run_creator_content_scene_v1,
    scan_plugin_inventory_v1, uninstall_user_plugin_v1, update_local_plugin_folder_v1, Artifact,
    ArtifactStore, AssetLibrarySnapshotV1, ComputeProviderConnectionState,
    ComputeProviderLivenessPolicyV1, ComputeProviderRuntime, ComputeProviderSchedulingSnapshotV1,
    ComputeRunningAssignmentV1, CreatorContentSceneOptionsV1, CreatorInputV1,
    CreatorProductionPackOptionsV1, CreatorRunCoordinatorV1, CreatorStockDiscoveryV1,
    CreatorVisualAssetExecutorV1, CreatorVisualDiscoveryExecutorV1,
    CreatorVisualGenerationRequestV1, CreatorVisualPlanningOptionsV1,
    CreatorVisualStockFetchRequestV1, CreatorVoiceRuntimeV1, DiscoveredPlugin, Error as CoreError,
    GeneratedImagePluginResultV1, GeneratedImageRequestV1, GeneratedImageResolutionV1,
    GeneratedImageStyleV1, GpuBatchBudgetOverviewV1, GpuBatchPlanRequestV1, GpuBatchPlanV1,
    GpuBurstDispatchSummaryV1, GpuBurstPlanV1, GpuJobPreparationV1, GpuWorkbenchQueueSnapshotV1,
    HandoffManifest, HttpComputeProvider, HttpComputeProviderConfigV1, LlmGatewayClient,
    LlmGatewayConfig, LlmGatewayModel, MachineBinding, PluginCapabilityImpactV1,
    PluginInventoryEntryV1, PluginInventoryReportV1, PluginJobWorkspace, PluginLifecycleStateV1,
    PluginMutationKindV1, PluginProcess, PluginProcessOptions, PluginRegistry, PluginResponse,
    PluginRuntimeReadinessV1, PluginUpdatePreviewV1, PortableStudioPackCatalogV1,
    ProductionExportHistoryEntryV1, ProductionPackV1, ProductionPackageExportOutcomeV1,
    ProductionPackageExporterV1, Project, ProjectBoardProjectionV1, ProjectDisplayStatus,
    RemoteComputeJobSpecV1, RemoteReconciliationSummaryV1, Result as CoreResult,
    RuntimeWorkloadEstimateV1, SegmentTtsLockStateV1, SelectedVisualOutput, StateStore,
    StockDiscoveryStatusV1, StudioAutomationLevelV1, StudioJobReviewSnapshotV1,
    StudioPackAvailabilityStatusV1, StudioPackOverridesV1, StudioPackRouteTargetV1,
    StudioPackRuntimeSnapshotV1, StudioPackUxViewV1, StudioPackV1, StudioReviewCenterV1,
    VisualCandidate, VisualCandidateRankingInput, VisualCandidateSignals, VoiceIdentityV1,
    VoiceModelIdentityV1, WorkflowStep, Workspace, WorkspaceSession,
    CREATOR_STEP_VISUAL_PREPARE_V1, STICK_FIGURE_VISUAL_CAPABILITY_V1, STUDIO_PACK_SCHEMA_V1,
    STUDIO_PACK_VERSION_V1,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

#[derive(Default)]
struct DesktopState {
    active: Mutex<Option<ActiveWorkspace>>,
    compute: Mutex<Option<ComputeProviderRuntime<HttpComputeProvider>>>,
}

enum ActiveWorkspace {
    Writable(WorkspaceSession),
    ReadOnly(Workspace),
}

impl ActiveWorkspace {
    fn workspace(&self) -> &Workspace {
        match self {
            Self::Writable(session) => session.workspace(),
            Self::ReadOnly(workspace) => workspace,
        }
    }

    fn is_read_only(&self) -> bool {
        matches!(self, Self::ReadOnly(_))
    }

    fn refresh_lease(&mut self) -> Result<(), String> {
        if let Self::Writable(session) = self {
            session.refresh_lease().map_err(error_string)?;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AppSnapshot {
    Unconfigured,
    Ready {
        workspace: WorkspaceView,
        projects: Vec<ProjectView>,
    },
    Conflict {
        data_root: String,
        message: String,
    },
    Unavailable {
        data_root: String,
        message: String,
    },
    HandoffReady {
        data_root: String,
        revision: u64,
        snapshot_sha256: String,
    },
}

#[derive(Debug, Serialize)]
struct PluginInventoryDesktopViewV1 {
    plugins: Vec<PluginInventoryEntryV1>,
    diagnostics: Vec<PluginDiagnosticDesktopViewV1>,
    readiness: Vec<PluginRuntimeReadinessDesktopViewV1>,
}

#[derive(Debug, Serialize)]
struct PluginRuntimeReadinessDesktopViewV1 {
    plugin_id: String,
    status: String,
    reason_code: Option<String>,
}

#[derive(Debug, Serialize)]
struct PluginDiagnosticDesktopViewV1 {
    code: String,
    path: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct WorkspaceView {
    data_root: String,
    workspace_id: String,
    revision: u64,
    last_clean_shutdown: bool,
    last_writer_device: Option<String>,
    read_only: bool,
}

#[derive(Debug, Serialize)]
struct ProjectView {
    project: Project,
    status: ProjectDisplayStatus,
    board: ProjectBoardProjectionV1,
    steps: Vec<WorkflowStep>,
    run: Option<CreatorRunCoordinatorV1>,
}

#[derive(Debug, Serialize)]
struct StudioPackCatalogDesktopViewV1 {
    packs: Vec<StudioPackCatalogItemDesktopViewV1>,
}

#[derive(Debug, Serialize)]
struct StudioPackCatalogItemDesktopViewV1 {
    custom: bool,
    pack: StudioPackUxViewV1,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum LlmGatewayConnectionState {
    Ready,
    NeedsApiKey,
    Offline,
    Degraded,
}

#[derive(Debug, Serialize)]
struct LlmGatewayStatusView {
    state: LlmGatewayConnectionState,
    base_url: String,
    api_key_env: String,
    default_model: String,
    credential_present: bool,
    health_status: Option<String>,
    gateway_default_model: Option<String>,
    models: Vec<LlmGatewayModelView>,
    message: String,
}

#[derive(Debug, Serialize)]
struct LlmGatewayModelView {
    id: String,
    display_name: String,
    is_virtual: bool,
}

#[derive(Debug, Deserialize)]
struct GpuWorkbenchPrepareInputV1 {
    project_ids: Vec<String>,
    #[serde(default)]
    preparations: Vec<GpuJobPreparationV1>,
    #[serde(default)]
    providers: Vec<ComputeProviderSchedulingSnapshotV1>,
    #[serde(default)]
    running: Vec<ComputeRunningAssignmentV1>,
    week_start: String,
}

#[derive(Debug, Serialize)]
struct GpuWorkbenchReviewViewV1 {
    batch: GpuBatchPlanV1,
    workload: RuntimeWorkloadEstimateV1,
    budget: Option<GpuBatchBudgetOverviewV1>,
    burst: GpuBurstPlanV1,
    queues: GpuWorkbenchQueueSnapshotV1,
    startable: bool,
}

#[derive(Debug, Deserialize)]
struct GpuBurstStartInputV1 {
    reviewed_batch: GpuBatchPlanV1,
    #[serde(default)]
    providers: Vec<ComputeProviderSchedulingSnapshotV1>,
    #[serde(default)]
    execution_specs: Vec<RemoteComputeJobSpecV1>,
    expected_schedule_hash: String,
}

#[derive(Debug, Serialize)]
struct GpuBurstStartViewV1 {
    dispatch: GpuBurstDispatchSummaryV1,
    reconciliation: RemoteReconciliationSummaryV1,
    queues: GpuWorkbenchQueueSnapshotV1,
}

#[derive(Debug, Serialize)]
struct ComputeProviderStatusViewV1 {
    state: String,
    provider_id: String,
    base_url: String,
    bearer_token_env: Option<String>,
    credential_present: bool,
    session_id: Option<String>,
    capabilities: Option<omnicreator_core::ComputeProviderCapabilitiesV1>,
    message: String,
}

#[derive(Debug, Serialize)]
struct ComputeBurstSyncViewV1 {
    provider: ComputeProviderStatusViewV1,
    reconciliation: RemoteReconciliationSummaryV1,
    queues: GpuWorkbenchQueueSnapshotV1,
}

#[derive(Debug, Serialize)]
struct ProductionExportDiagnosticViewV1 {
    kind: String,
    artifact_id: Option<String>,
    logical_uri: Option<String>,
    message: String,
    action: String,
}

#[derive(Debug, Serialize)]
struct ProductionExportViewV1 {
    project_id: String,
    state: String,
    outcome: Option<ProductionPackageExportOutcomeV1>,
    history: Vec<ProductionExportHistoryEntryV1>,
    last_pack: Option<ProductionPackV1>,
    diagnostic: Option<ProductionExportDiagnosticViewV1>,
}

#[tauri::command]
fn pick_data_root() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Select OmniCreator Data Folder")
        .pick_folder()
        .map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command]
fn pick_plugin_folder() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Select OmniCreator Plugin Folder")
        .pick_folder()
        .map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command]
fn bootstrap(app: AppHandle, state: State<'_, DesktopState>) -> Result<AppSnapshot, String> {
    if state.active.lock().map_err(lock_error)?.is_some() {
        return snapshot_from_active(&state);
    }

    let binding_path = binding_path(&app)?;
    if !binding_path.exists() {
        return Ok(AppSnapshot::Unconfigured);
    }

    let binding = MachineBinding::load(&binding_path).map_err(error_string)?;
    if !binding.data_root.exists() {
        return Ok(AppSnapshot::Unavailable {
            data_root: path_text(&binding.data_root),
            message: "The configured Data Folder is not available on this machine.".to_owned(),
        });
    }

    open_path(
        &app,
        &state,
        binding.data_root,
        binding.device_id,
        false,
        false,
    )
}

#[tauri::command]
fn create_data_root(
    app: AppHandle,
    state: State<'_, DesktopState>,
    data_root: String,
) -> Result<AppSnapshot, String> {
    let path = absolute_path(&data_root)?;
    let device_id = local_device_id(&app)?;
    open_path(&app, &state, path, device_id, true, false)
}

#[tauri::command]
fn use_existing_data_root(
    app: AppHandle,
    state: State<'_, DesktopState>,
    data_root: String,
) -> Result<AppSnapshot, String> {
    let path = absolute_path(&data_root)?;
    let device_id = local_device_id(&app)?;
    open_path(&app, &state, path, device_id, false, false)
}

#[tauri::command]
fn open_read_only(
    app: AppHandle,
    state: State<'_, DesktopState>,
    data_root: String,
) -> Result<AppSnapshot, String> {
    let path = absolute_path(&data_root)?;
    let device_id = local_device_id(&app)?;
    open_path(&app, &state, path, device_id, false, true)
}

#[tauri::command]
fn check_again(
    app: AppHandle,
    state: State<'_, DesktopState>,
    data_root: String,
) -> Result<AppSnapshot, String> {
    let path = absolute_path(&data_root)?;
    let device_id = local_device_id(&app)?;
    open_path(&app, &state, path, device_id, false, false)
}

#[tauri::command]
fn heartbeat(state: State<'_, DesktopState>) -> Result<(), String> {
    let mut guard = state.active.lock().map_err(lock_error)?;
    if let Some(active) = guard.as_mut() {
        active.refresh_lease()?;
    }
    Ok(())
}

#[tauri::command]
fn list_projects(state: State<'_, DesktopState>) -> Result<AppSnapshot, String> {
    snapshot_from_active(&state)
}

#[tauri::command]
fn create_project(state: State<'_, DesktopState>, title: String) -> Result<AppSnapshot, String> {
    if title.trim().is_empty() {
        return Err("Project title must not be empty.".to_owned());
    }

    with_writable_store(&state, |store| {
        store.create_project(title.trim()).map(|_| ())
    })?;
    snapshot_from_active(&state)
}

#[tauri::command]
fn rename_project(
    state: State<'_, DesktopState>,
    project_id: String,
    title: String,
) -> Result<AppSnapshot, String> {
    if title.trim().is_empty() {
        return Err("Project title must not be empty.".to_owned());
    }

    with_writable_store(&state, |store| {
        store
            .update_project_title(&project_id, title.trim())
            .map(|_| ())
    })?;
    snapshot_from_active(&state)
}

#[tauri::command]
fn delete_project(
    state: State<'_, DesktopState>,
    project_id: String,
) -> Result<AppSnapshot, String> {
    with_writable_store(&state, |store| store.delete_project(&project_id))?;
    snapshot_from_active(&state)
}

#[tauri::command]
fn plugin_inventory(app: AppHandle) -> Result<PluginInventoryDesktopViewV1, String> {
    let report = plugin_inventory_report_v1(&app)?;
    let runtime = studio_pack_runtime_snapshot_v1(&app, &report.registry)?;
    let readiness = report
        .inventory
        .iter()
        .map(|plugin| {
            let (status, reason_code) = match runtime.get_v1(&plugin.id) {
                PluginRuntimeReadinessV1::Ready => ("ready".to_owned(), None),
                PluginRuntimeReadinessV1::SetupRequired { reason_code } => {
                    ("setup_required".to_owned(), Some(reason_code))
                }
                PluginRuntimeReadinessV1::Unavailable { reason_code } => {
                    ("unavailable".to_owned(), Some(reason_code))
                }
            };
            PluginRuntimeReadinessDesktopViewV1 {
                plugin_id: plugin.id.clone(),
                status,
                reason_code,
            }
        })
        .collect();

    Ok(PluginInventoryDesktopViewV1 {
        plugins: report.inventory,
        diagnostics: report
            .diagnostics
            .into_iter()
            .map(|diagnostic| PluginDiagnosticDesktopViewV1 {
                code: diagnostic.code.as_str().to_owned(),
                path: diagnostic.path.to_string_lossy().into_owned(),
                message: diagnostic.message,
            })
            .collect(),
        readiness,
    })
}

#[tauri::command]
fn set_plugin_enabled(
    app: AppHandle,
    plugin_id: String,
    enabled: bool,
) -> Result<PluginInventoryDesktopViewV1, String> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() {
        return Err("Plugin id must not be empty.".to_owned());
    }

    let current = plugin_inventory_report_v1(&app)?;
    if current.registry.get(plugin_id).is_none() {
        return Err(format!(
            "Plugin {plugin_id} is not installed on this machine."
        ));
    }

    let path = plugin_lifecycle_path_v1(&app)?;
    let mut lifecycle = PluginLifecycleStateV1::load_v1(&path).map_err(error_string)?;
    lifecycle
        .set_enabled_v1(plugin_id, enabled)
        .map_err(error_string)?;
    lifecycle.save_v1(path).map_err(error_string)?;
    plugin_inventory(app)
}

#[tauri::command]
fn install_plugin_from_folder(
    app: AppHandle,
    source_path: String,
) -> Result<PluginInventoryDesktopViewV1, String> {
    let source_path = source_path.trim();
    if source_path.is_empty() {
        return Err("Plugin source path must not be empty.".to_owned());
    }

    let built_in_roots = plugin_built_in_roots_v1(&app);
    let user_root = plugin_user_root_v1(&app)?;
    install_local_plugin_folder_v1(Path::new(source_path), &built_in_roots, &user_root)
        .map_err(error_string)?;
    plugin_inventory(app)
}

#[tauri::command]
fn uninstall_plugin(
    app: AppHandle,
    plugin_id: String,
) -> Result<PluginInventoryDesktopViewV1, String> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() {
        return Err("Plugin id must not be empty.".to_owned());
    }

    let built_in_roots = plugin_built_in_roots_v1(&app);
    let user_root = plugin_user_root_v1(&app)?;
    uninstall_user_plugin_v1(plugin_id, &built_in_roots, &user_root).map_err(error_string)?;
    plugin_inventory(app)
}

#[tauri::command]
fn inspect_plugin_update(
    app: AppHandle,
    plugin_id: String,
    source_path: String,
) -> Result<PluginUpdatePreviewV1, String> {
    let plugin_id = plugin_id.trim();
    let source_path = source_path.trim();
    if plugin_id.is_empty() {
        return Err("Plugin id must not be empty.".to_owned());
    }
    if source_path.is_empty() {
        return Err("Plugin update source path must not be empty.".to_owned());
    }

    let built_in_roots = plugin_built_in_roots_v1(&app);
    let user_root = plugin_user_root_v1(&app)?;
    inspect_local_plugin_update_v1(
        plugin_id,
        Path::new(source_path),
        &built_in_roots,
        &user_root,
    )
    .map_err(error_string)
}

#[tauri::command]
fn apply_plugin_update(
    app: AppHandle,
    plugin_id: String,
    source_path: String,
) -> Result<PluginInventoryDesktopViewV1, String> {
    let plugin_id = plugin_id.trim();
    let source_path = source_path.trim();
    if plugin_id.is_empty() {
        return Err("Plugin id must not be empty.".to_owned());
    }
    if source_path.is_empty() {
        return Err("Plugin update source path must not be empty.".to_owned());
    }

    let built_in_roots = plugin_built_in_roots_v1(&app);
    let user_root = plugin_user_root_v1(&app)?;
    update_local_plugin_folder_v1(
        plugin_id,
        Path::new(source_path),
        &built_in_roots,
        &user_root,
    )
    .map_err(error_string)?;
    plugin_inventory(app)
}

#[tauri::command]
fn plugin_mutation_impact(
    app: AppHandle,
    state: State<'_, DesktopState>,
    plugin_id: String,
    mutation: String,
    source_path: Option<String>,
) -> Result<PluginCapabilityImpactV1, String> {
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() {
        return Err("Plugin id must not be empty.".to_owned());
    }

    let mutation = match mutation.trim() {
        "disable" => PluginMutationKindV1::Disable,
        "remove" => PluginMutationKindV1::Remove,
        "update" => PluginMutationKindV1::Update,
        other => {
            return Err(format!(
                "Unsupported plugin mutation '{other}'. Expected disable, remove, or update."
            ))
        }
    };

    let built_in_roots = plugin_built_in_roots_v1(&app);
    let user_root = plugin_user_root_v1(&app)?;
    let lifecycle = load_plugin_lifecycle_v1(&app)?;
    let report = plugin_inventory_report_v1(&app)?;
    let update_preview = if mutation == PluginMutationKindV1::Update {
        let source_path = source_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "Update impact preview requires a local source path.".to_owned())?;
        Some(
            inspect_local_plugin_update_v1(
                plugin_id,
                Path::new(source_path),
                &built_in_roots,
                &user_root,
            )
            .map_err(error_string)?,
        )
    } else {
        None
    };

    let (catalog, projects) = plugin_impact_context_v1(&state)?;
    preview_plugin_capability_impact_v1(
        &report.registry,
        &lifecycle,
        &catalog,
        &projects,
        plugin_id,
        mutation,
        update_preview.as_ref(),
    )
    .map_err(error_string)
}

#[tauri::command]
fn studio_pack_catalog(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<StudioPackCatalogDesktopViewV1, String> {
    studio_pack_catalog_view_v1(&app, &state)
}

#[tauri::command]
fn create_project_from_studio_pack(
    app: AppHandle,
    state: State<'_, DesktopState>,
    title: String,
    pack_id: String,
    overrides: StudioPackOverridesV1,
) -> Result<AppSnapshot, String> {
    if title.trim().is_empty() {
        return Err("Project title must not be empty.".to_owned());
    }

    let data_root = active_data_root(&state)?;
    let mut catalog = load_studio_pack_catalog_v1(&data_root)?;
    validate_desktop_studio_pack_overrides_v1(&catalog, &pack_id, &overrides)?;

    let registry = studio_pack_plugin_registry_v1(&app)?;
    let runtime = studio_pack_runtime_snapshot_v1(&app, &registry)?;
    let selected_id = if overrides == StudioPackOverridesV1::default() {
        pack_id.clone()
    } else {
        let base = catalog
            .list_definitions_v1()
            .map_err(error_string)?
            .into_iter()
            .find(|pack| pack.id == pack_id)
            .ok_or_else(|| format!("Studio Pack not found: {pack_id}"))?;
        let custom_id = format!("project-custom-{}", Uuid::new_v4().simple());
        let custom = StudioPackV1 {
            schema: STUDIO_PACK_SCHEMA_V1.to_owned(),
            schema_version: STUDIO_PACK_VERSION_V1,
            id: custom_id.clone(),
            name: format!("{} Custom", base.name),
            extends: Some(pack_id.clone()),
            overrides,
        };
        let mut packs = catalog.packs.clone();
        packs.push(custom);
        catalog = PortableStudioPackCatalogV1::from_packs_v1(packs).map_err(error_string)?;
        save_studio_pack_catalog_v1(&data_root, &catalog)?;
        custom_id
    };

    let availability = catalog
        .evaluate_availability_v1(&selected_id, &registry, &runtime)
        .map_err(error_string)?;
    if availability.status != StudioPackAvailabilityStatusV1::Available {
        return Err(format!(
            "Studio Pack {selected_id} is not ready to create a project: {:?}",
            availability.status
        ));
    }

    let store = writable_store(&state)?;
    let effective = catalog.resolve_v1(&selected_id).map_err(error_string)?;
    let project = store
        .create_project_with_studio_pack(title.trim(), Some(&selected_id))
        .map_err(error_string)?;
    let workflow = compile_creator_workflow_plan_v1(&project, &effective).map_err(error_string)?;
    materialize_creator_workflow_plan_v1(&store, &workflow).map_err(error_string)?;
    snapshot_from_active(&state)
}

#[tauri::command]
fn update_project_studio_pack(
    app: AppHandle,
    state: State<'_, DesktopState>,
    project_id: String,
    base_pack_id: String,
    overrides: StudioPackOverridesV1,
) -> Result<AppSnapshot, String> {
    let data_root = active_data_root(&state)?;
    let mut catalog = load_studio_pack_catalog_v1(&data_root)?;
    validate_desktop_studio_pack_overrides_v1(&catalog, &base_pack_id, &overrides)?;

    let store = writable_store(&state)?;
    let project = store.get_project(&project_id).map_err(error_string)?;
    let selected_id = if overrides == StudioPackOverridesV1::default() {
        if let Some(custom_id) = project
            .studio_pack
            .as_deref()
            .filter(|id| id.starts_with("project-custom-"))
        {
            let packs = catalog
                .packs
                .iter()
                .filter(|pack| pack.id != custom_id)
                .cloned()
                .collect::<Vec<_>>();
            catalog = PortableStudioPackCatalogV1::from_packs_v1(packs).map_err(error_string)?;
            save_studio_pack_catalog_v1(&data_root, &catalog)?;
        }
        base_pack_id.clone()
    } else {
        let custom_id = project
            .studio_pack
            .as_deref()
            .filter(|id| id.starts_with("project-custom-"))
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("project-custom-{}", project.id));
        let base = catalog
            .list_definitions_v1()
            .map_err(error_string)?
            .into_iter()
            .find(|pack| pack.id == base_pack_id)
            .ok_or_else(|| format!("Studio Pack not found: {base_pack_id}"))?;
        let custom = StudioPackV1 {
            schema: STUDIO_PACK_SCHEMA_V1.to_owned(),
            schema_version: STUDIO_PACK_VERSION_V1,
            id: custom_id.clone(),
            name: format!("{} Custom", base.name),
            extends: Some(base_pack_id.clone()),
            overrides,
        };
        let mut packs = catalog
            .packs
            .iter()
            .filter(|pack| pack.id != custom_id)
            .cloned()
            .collect::<Vec<_>>();
        packs.push(custom);
        catalog = PortableStudioPackCatalogV1::from_packs_v1(packs).map_err(error_string)?;
        save_studio_pack_catalog_v1(&data_root, &catalog)?;
        custom_id
    };

    let registry = studio_pack_plugin_registry_v1(&app)?;
    let runtime = studio_pack_runtime_snapshot_v1(&app, &registry)?;
    let availability = catalog
        .evaluate_availability_v1(&selected_id, &registry, &runtime)
        .map_err(error_string)?;
    if availability.status != StudioPackAvailabilityStatusV1::Available {
        return Err(format!(
            "Studio Pack {selected_id} is not ready for this project: {:?}",
            availability.status
        ));
    }

    store
        .update_project_studio_pack(&project_id, Some(&selected_id))
        .map_err(error_string)?;
    snapshot_from_active(&state)
}

#[tauri::command]
fn asset_library(state: State<'_, DesktopState>) -> Result<AssetLibrarySnapshotV1, String> {
    readable_store(&state)?
        .asset_library_snapshot_v1(Utc::now())
        .map_err(error_string)
}

#[tauri::command]
fn add_asset_tag(
    state: State<'_, DesktopState>,
    artifact_id: String,
    tag: String,
) -> Result<AssetLibrarySnapshotV1, String> {
    let store = writable_store(&state)?;
    store
        .add_asset_tag_v1(&artifact_id, &tag)
        .map_err(error_string)?;
    store
        .asset_library_snapshot_v1(Utc::now())
        .map_err(error_string)
}

#[tauri::command]
fn remove_asset_tag(
    state: State<'_, DesktopState>,
    artifact_id: String,
    tag: String,
) -> Result<AssetLibrarySnapshotV1, String> {
    let store = writable_store(&state)?;
    store
        .remove_asset_tag_v1(&artifact_id, &tag)
        .map_err(error_string)?;
    store
        .asset_library_snapshot_v1(Utc::now())
        .map_err(error_string)
}

struct DesktopVisualRuntimeV1<'a> {
    registry: &'a PluginRegistry,
    runtime: &'a StudioPackRuntimeSnapshotV1,
    runtime_root: PathBuf,
}

impl DesktopVisualRuntimeV1<'_> {
    fn resolve_plugin_v1(
        &self,
        target: &StudioPackRouteTargetV1,
        preferred_provider: Option<&str>,
    ) -> CoreResult<&DiscoveredPlugin> {
        let ready = |plugin_id: &str| {
            matches!(
                self.runtime.get_v1(plugin_id),
                PluginRuntimeReadinessV1::Ready
            )
        };

        if let Some(plugin_id) = preferred_provider.or(target.plugin_id.as_deref()) {
            let plugin = self.registry.get(plugin_id).ok_or_else(|| {
                CoreError::InvalidContract(format!("visual plugin {plugin_id} is not installed"))
            })?;
            if !ready(&plugin.manifest.id) {
                return Err(CoreError::InvalidContract(format!(
                    "visual plugin {} is not runtime-ready",
                    plugin.manifest.id
                )));
            }
            if !plugin
                .manifest
                .capabilities
                .iter()
                .any(|capability| capability == &target.capability)
            {
                return Err(CoreError::InvalidContract(format!(
                    "visual plugin {} does not provide {}",
                    plugin.manifest.id, target.capability
                )));
            }
            return Ok(plugin);
        }

        self.registry
            .plugin_ids_for_capability(&target.capability)
            .iter()
            .filter(|plugin_id| ready(plugin_id))
            .find_map(|plugin_id| self.registry.get(plugin_id))
            .ok_or_else(|| {
                CoreError::InvalidContract(format!(
                    "no runtime-ready visual plugin provides {}",
                    target.capability
                ))
            })
    }

    fn process_result_v1(
        plugin: &DiscoveredPlugin,
        response: PluginResponse,
        operation: &str,
    ) -> CoreResult<serde_json::Value> {
        match response {
            PluginResponse::Success { result, .. } => Ok(result),
            PluginResponse::Failure { error, .. } => Err(CoreError::PluginProtocol {
                plugin: plugin.manifest.id.clone(),
                message: format!("{operation} failed with {}: {}", error.code, error.message),
            }),
        }
    }

    fn deterministic_signals_v1(
        candidate: &VisualCandidate,
        ordinal: usize,
    ) -> VisualCandidateSignals {
        let relevance = (0.96 - ordinal as f64 * 0.025).clamp(0.72, 0.96);
        let quality = match (candidate.width, candidate.height) {
            (Some(width), Some(height)) if width >= 1920 || height >= 1080 => 0.92,
            (Some(width), Some(height)) if width >= 1280 || height >= 720 => 0.86,
            _ => 0.78,
        };
        let editability = if candidate.media_type == omnicreator_core::VisualMediaType::Video {
            0.88
        } else {
            0.82
        };
        VisualCandidateSignals {
            semantic_relevance: relevance,
            emotional_relevance: (relevance - 0.04).max(0.0),
            narrative_purpose: (relevance - 0.02).max(0.0),
            visual_quality: quality,
            channel_continuity: 0.80,
            editability,
            usage_count: 0,
            used_recently: false,
        }
    }

    fn target_uri_v1(
        scene_id: &str,
        job_id: &str,
        relative_output: &str,
    ) -> CoreResult<omnicreator_core::LogicalUri> {
        let extension = Path::new(relative_output)
            .extension()
            .and_then(|value| value.to_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("bin");
        omnicreator_core::LogicalUri::parse(&format!(
            "project://visual/{scene_id}/{job_id}.{extension}"
        ))
    }

    fn finish_attempt_for_error_v1(
        state_store: &mut StateStore,
        attempt_id: &str,
        error: &CoreError,
    ) {
        let code = match error {
            CoreError::PluginProtocol { .. } => "PROVIDER_UNAVAILABLE",
            CoreError::PluginTimeout { .. } => "NETWORK_TIMEOUT",
            CoreError::PluginProcessExited { .. } | CoreError::PluginSpawn { .. } => {
                "LOCAL_RUNTIME_CONTEXT_ERROR"
            }
            _ => "LOCAL_RUNTIME_CONTEXT_ERROR",
        };
        let _ = state_store.finish_attempt_failure(attempt_id, code);
    }
}

impl CreatorVisualDiscoveryExecutorV1 for DesktopVisualRuntimeV1<'_> {
    fn discover_stock_v1(
        &self,
        scene: &omnicreator_core::SceneIntentV1,
        ordered_targets: &[StudioPackRouteTargetV1],
    ) -> CoreResult<CreatorStockDiscoveryV1> {
        let mut ranking_inputs = Vec::new();
        let mut seen = BTreeSet::new();
        let mut any_runtime_ready = false;

        for target in ordered_targets {
            let Ok(plugin) = self.resolve_plugin_v1(target, target.plugin_id.as_deref()) else {
                continue;
            };
            any_runtime_ready = true;

            let process = match PluginProcess::spawn(plugin, PluginProcessOptions::default()) {
                Ok(process) => process,
                Err(_) => continue,
            };
            let initialized = process.initialize(serde_json::json!({}));
            if initialized
                .ok()
                .and_then(|call| {
                    Self::process_result_v1(plugin, call.response, "plugin.initialize").ok()
                })
                .is_none()
            {
                let _ = process.shutdown();
                continue;
            }

            let media_type = match target.capability.as_str() {
                "stock_image" => "image",
                "stock_video" => "video",
                _ => {
                    let _ = process.shutdown();
                    continue;
                }
            };
            let call = process.execute(
                "visual.resolve",
                serde_json::json!({
                    "scene": scene,
                    "media_type": media_type,
                }),
            );
            let result = match call.ok().and_then(|call| {
                Self::process_result_v1(plugin, call.response, "visual.resolve").ok()
            }) {
                Some(result) => result,
                None => {
                    let _ = process.shutdown();
                    continue;
                }
            };
            let _ = process.shutdown();

            let candidates = result
                .get("candidates")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            for raw in candidates {
                let candidate: VisualCandidate = match serde_json::from_value(raw) {
                    Ok(candidate) => candidate,
                    Err(_) => continue,
                };
                if candidate.validate().is_err() || !seen.insert(candidate.candidate_id.clone()) {
                    continue;
                }
                let ordinal = ranking_inputs.len();
                ranking_inputs.push(VisualCandidateRankingInput {
                    signals: Self::deterministic_signals_v1(&candidate, ordinal),
                    candidate,
                });
            }
        }

        Ok(CreatorStockDiscoveryV1 {
            status: if !ranking_inputs.is_empty() || any_runtime_ready {
                StockDiscoveryStatusV1::Complete
            } else {
                StockDiscoveryStatusV1::Unavailable
            },
            ranking_inputs,
        })
    }
}

impl CreatorVisualAssetExecutorV1 for DesktopVisualRuntimeV1<'_> {
    fn fetch_selected_stock_v1(
        &self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        request: CreatorVisualStockFetchRequestV1<'_>,
    ) -> CoreResult<Artifact> {
        let plugin = self.resolve_plugin_v1(
            request.target,
            Some(request.candidate.source_provider.as_str()),
        )?;
        let workspace = PluginJobWorkspace::create(&self.runtime_root, request.job_id)?;
        let process = PluginProcess::spawn(plugin, PluginProcessOptions::default())?;
        let initialize = process.initialize(workspace.initialization_context(plugin)?)?;
        Self::process_result_v1(plugin, initialize.response, "plugin.initialize")?;

        let started = state_store.start_attempt(
            request.job_id,
            Some(&format!("plugin:{}", plugin.manifest.id)),
        )?;
        let result = (|| {
            let call = process.execute(
                "visual.fetch_selected",
                serde_json::json!({
                    "selection_ref": request.candidate.selection_ref,
                    "quality_mode": "standard",
                }),
            )?;
            let value = Self::process_result_v1(plugin, call.response, "visual.fetch_selected")?;
            let selected: SelectedVisualOutput = serde_json::from_value(value)?;
            selected.validate()?;
            if selected.source_provider != request.candidate.source_provider
                || selected.source_asset_id != request.candidate.source_asset_id
                || selected.selection_ref != request.candidate.selection_ref
            {
                return Err(CoreError::InvalidArtifact(
                    "selected stock plugin output does not match reviewed candidate".to_owned(),
                ));
            }
            let verified = workspace.verify_output_file(&selected.relative_output)?;
            let mut promotion = selected.promotion(Self::target_uri_v1(
                &request.scene.id,
                request.job_id,
                &selected.relative_output,
            )?)?;
            let metadata = promotion.metadata.as_object_mut().ok_or_else(|| {
                CoreError::InvalidArtifact("selected visual metadata must be an object".to_owned())
            })?;
            metadata.insert(
                "route_target".to_owned(),
                serde_json::to_value(request.target)?,
            );
            metadata.insert(
                "visual_routing".to_owned(),
                serde_json::to_value(request.routing)?,
            );

            let artifacts = artifact_store.promote_attempt_outputs(
                state_store,
                omnicreator_core::artifact_store::AttemptPromotionRequest {
                    attempt_id: started.attempt_id.clone(),
                    job_id: request.job_id.to_owned(),
                    outputs: vec![omnicreator_core::artifact_store::AttemptOutputPromotion {
                        source: verified.path().to_path_buf(),
                        target_uri: promotion.target_uri,
                        artifact_type: promotion.artifact_type,
                        metadata: promotion.metadata,
                        expected_sha256: None,
                    }],
                    selected_output_index: 0,
                },
            )?;
            artifacts.into_iter().next().ok_or_else(|| {
                CoreError::InvalidArtifact(
                    "selected stock plugin produced no promoted artifact".to_owned(),
                )
            })
        })();

        let _ = process.shutdown();
        match result {
            Ok(artifact) => Ok(artifact),
            Err(error) => {
                Self::finish_attempt_for_error_v1(state_store, &started.attempt_id, &error);
                Err(error)
            }
        }
    }

    fn generate_visual_v1(
        &self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        request: CreatorVisualGenerationRequestV1<'_>,
    ) -> CoreResult<Artifact> {
        let plugin = self.resolve_plugin_v1(request.target, request.target.plugin_id.as_deref())?;
        let workspace = PluginJobWorkspace::create(&self.runtime_root, request.job_id)?;
        let process = PluginProcess::spawn(plugin, PluginProcessOptions::default())?;
        let initialize = process.initialize(workspace.initialization_context(plugin)?)?;
        Self::process_result_v1(plugin, initialize.response, "plugin.initialize")?;

        let style = GeneratedImageStyleV1 {
            preset: request
                .target
                .preset
                .clone()
                .unwrap_or_else(|| "default".to_owned()),
            description: None,
        };
        let seed = request.scene.id.bytes().fold(0_u64, |value, byte| {
            value.wrapping_mul(131).wrapping_add(u64::from(byte))
        });
        let generated = GeneratedImageRequestV1::from_scene_v1(
            request.scene.clone(),
            style,
            GeneratedImageResolutionV1 {
                width: 1280,
                height: 720,
            },
            Some(seed),
            BTreeMap::new(),
        )?;

        let started = state_store.start_attempt(
            request.job_id,
            Some(&format!("plugin:{}", plugin.manifest.id)),
        )?;
        let result = (|| {
            let call = process.execute("visual.generate", serde_json::to_value(&generated)?)?;
            let value = Self::process_result_v1(plugin, call.response, "visual.generate")?;
            let generated_result: GeneratedImagePluginResultV1 = serde_json::from_value(value)?;
            let verified = workspace.verify_output_file(&generated_result.relative_output)?;

            let target_uri = Self::target_uri_v1(
                &request.scene.id,
                request.job_id,
                &generated_result.relative_output,
            )?;
            let artifacts = artifact_store.promote_attempt_outputs(
                state_store,
                omnicreator_core::artifact_store::AttemptPromotionRequest {
                    attempt_id: started.attempt_id.clone(),
                    job_id: request.job_id.to_owned(),
                    outputs: vec![omnicreator_core::artifact_store::AttemptOutputPromotion {
                        source: verified.path().to_path_buf(),
                        target_uri,
                        artifact_type: "image".to_owned(),
                        metadata: serde_json::json!({
                            "source_provider": plugin.manifest.id,
                            "capability": request.target.capability,
                            "route_target": request.target,
                            "visual_routing": request.routing,
                            "model": {
                                "id": generated_result.model_id,
                                "version": generated_result.model_version,
                            },
                            "seed": generated_result.seed,
                            "prompt_sha256": generated_result.prompt_sha256,
                            "settings_fingerprint": generated_result.settings_fingerprint,
                            "provider_metadata": generated_result.metadata,
                            "provenance": generated_result.provenance,
                        }),
                        expected_sha256: Some(generated_result.sha256),
                    }],
                    selected_output_index: 0,
                },
            )?;
            artifacts.into_iter().next().ok_or_else(|| {
                CoreError::InvalidArtifact(
                    "generated visual plugin produced no promoted artifact".to_owned(),
                )
            })
        })();

        let _ = process.shutdown();
        match result {
            Ok(artifact) => Ok(artifact),
            Err(error) => {
                Self::finish_attempt_for_error_v1(state_store, &started.attempt_id, &error);
                Err(error)
            }
        }
    }
}

#[tauri::command]
fn review_center(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<StudioReviewCenterV1, String> {
    studio_review_center_view_v1(&app, &state)
}

#[tauri::command]
fn retry_review_job(
    app: AppHandle,
    state: State<'_, DesktopState>,
    job_id: String,
) -> Result<StudioReviewCenterV1, String> {
    let mut store = writable_store(&state)?;
    store.prepare_job_retry(&job_id).map_err(error_string)?;
    drop(store);
    studio_review_center_view_v1(&app, &state)
}

fn creator_plugin_runtime_root_v1(app: &AppHandle) -> Result<PathBuf, String> {
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("Cannot resolve app cache directory: {error}"))?;
    let root = cache_dir.join("creator-plugin-runtime");
    fs::create_dir_all(&root)
        .map_err(|error| format!("Cannot create creator plugin runtime directory: {error}"))?;
    Ok(root)
}

fn creator_input_v1(input_kind: &str, input_text: &str) -> Result<Option<CreatorInputV1>, String> {
    let input_text = input_text.trim();
    if input_text.is_empty() {
        return Ok(None);
    }
    match input_kind.trim().to_ascii_uppercase().as_str() {
        "TOPIC" => Ok(Some(CreatorInputV1::topic(input_text))),
        "SCRIPT" => Ok(Some(CreatorInputV1::script(input_text))),
        _ => Err("Creator input kind must be TOPIC or SCRIPT.".to_owned()),
    }
}

fn creator_voice_runtime_v1(
    app: &AppHandle,
    pack: &omnicreator_core::EffectiveStudioPackV1,
    provider: Option<&ComputeProviderSchedulingSnapshotV1>,
) -> Result<CreatorVoiceRuntimeV1, String> {
    let provider_id = provider
        .map(|value| value.session.identity.provider_id.clone())
        .unwrap_or(load_compute_provider_config(app)?.provider_id);
    let model_group = provider
        .and_then(|value| {
            value
                .session
                .capabilities
                .model_groups
                .iter()
                .find(|group| group.to_ascii_lowercase().contains("omnivoice"))
                .cloned()
                .or_else(|| value.session.capabilities.model_groups.first().cloned())
        })
        .unwrap_or_else(|| "omnivoice".to_owned());

    let env_or = |name: &str, fallback: String| {
        env::var(name)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback)
    };
    let plugin_id = env_or("OMNICREATOR_VOICE_PLUGIN_ID", "omnivoice".to_owned());
    let voice_id = env_or(
        "OMNICREATOR_VOICE_ID",
        pack.config
            .presets
            .get("voice")
            .cloned()
            .unwrap_or_else(|| "warm-narrator".to_owned()),
    );
    let voice_version = env_or("OMNICREATOR_VOICE_VERSION", "1".to_owned());
    let model_id = env_or("OMNICREATOR_VOICE_MODEL_ID", model_group.clone());
    let model_version = env_or("OMNICREATOR_VOICE_MODEL_VERSION", "1".to_owned());
    let fingerprint = omnicreator_core::deterministic_input_hash(&[
        b"desktop-creator-voice-runtime-v1",
        provider_id.as_bytes(),
        plugin_id.as_bytes(),
        voice_id.as_bytes(),
        voice_version.as_bytes(),
        model_id.as_bytes(),
        model_version.as_bytes(),
    ]);

    Ok(CreatorVoiceRuntimeV1 {
        plugin_id,
        provider_id,
        voice: VoiceIdentityV1 {
            voice_id,
            voice_version,
        },
        model: VoiceModelIdentityV1 {
            model_id,
            model_version,
        },
        settings_fingerprint: fingerprint,
        pronunciation_rules: Vec::new(),
        locks: SegmentTtsLockStateV1 {
            normalization_locked: true,
            pronunciation_locked: true,
        },
        approval_required: false,
        approval_complete: true,
        production_lock_required: false,
        gpu_execution_requested: true,
        requirements: default_segment_tts_compute_requirements_v1(model_group, 12_000),
    })
}

#[tauri::command]
fn start_creator_production(
    app: AppHandle,
    state: State<'_, DesktopState>,
    project_id: String,
    input_kind: String,
    input_text: String,
) -> Result<AppSnapshot, String> {
    let requested_input = creator_input_v1(&input_kind, &input_text)?;
    let data_root = active_data_root(&state)?;
    let artifacts = ArtifactStore::new(&data_root).map_err(error_string)?;
    let catalog = load_studio_pack_catalog_v1(&data_root)?;
    let inventory = plugin_inventory_report_v1(&app)?;
    let plugin_runtime = studio_pack_runtime_snapshot_v1(&app, &inventory.registry)?;
    let runtime_root = creator_plugin_runtime_root_v1(&app)?;

    let mut store = writable_store(&state)?;
    let project = store.get_project(&project_id).map_err(error_string)?;
    let pack_id = project
        .studio_pack
        .as_deref()
        .ok_or_else(|| "Bind a Studio Pack before starting creator production.".to_owned())?;
    let pack = catalog.resolve_v1(pack_id).map_err(error_string)?;

    let mut creator = load_latest_creator_content_scene_v1(&store, &artifacts, &project_id)
        .map_err(error_string)?;
    let should_run_content = match (&requested_input, &creator) {
        (Some(input), Some(existing)) => existing.content.source != *input,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => {
            return Err(
                "Creator topic or script is required until Content + SceneIntent are prepared."
                    .to_owned(),
            )
        }
    };
    if should_run_content {
        let input = requested_input
            .as_ref()
            .expect("creator input is present when content must run");
        let llm = LlmGatewayClient::new(load_llmgateway_config(&app)?).map_err(error_string)?;
        creator = Some(
            run_creator_content_scene_v1(
                &mut store,
                &artifacts,
                &llm,
                &project_id,
                input,
                &CreatorContentSceneOptionsV1::default(),
            )
            .map_err(error_string)?,
        );
    }
    let creator = creator.ok_or_else(|| {
        "Creator content state is unavailable after content orchestration.".to_owned()
    })?;

    let visual_step = store
        .list_project_steps(&project_id)
        .map_err(error_string)?
        .into_iter()
        .find(|step| {
            step.step == CREATOR_STEP_VISUAL_PREPARE_V1
                && step.unit == omnicreator_core::CREATOR_WORKFLOW_UNIT_PROJECT_V1
        })
        .ok_or_else(|| "Creator visual workflow step is missing.".to_owned())?;
    if visual_step.status != omnicreator_core::StepStatus::Succeeded {
        let visual_runtime = DesktopVisualRuntimeV1 {
            registry: &inventory.registry,
            runtime: &plugin_runtime,
            runtime_root,
        };
        let visual_plan = plan_creator_visuals_v1(
            &project,
            &pack,
            &creator.content,
            &creator.scene_plan,
            &creator.scene_plan_artifact.sha256,
            &visual_runtime,
            &CreatorVisualPlanningOptionsV1::default(),
        )
        .map_err(error_string)?;
        let visual = execute_creator_visual_plan_v1(
            &mut store,
            &artifacts,
            &visual_plan,
            &creator.scene_plan,
            &visual_runtime,
        )
        .map_err(error_string)?;
        if !visual.completed {
            drop(store);
            return snapshot_from_active(&state);
        }
    }

    let provider_snapshot = {
        let mut guard = state.compute.lock().map_err(lock_error)?;
        if let Some(runtime) = guard.as_mut() {
            let _ = runtime.heartbeat(Utc::now());
            let connection_state = runtime.state();
            if let Some(session) = runtime.session().cloned() {
                let _ = reconcile_remote_session_v1(
                    &mut store,
                    &artifacts,
                    runtime.provider_mut(),
                    &session.identity.provider_id,
                    &session.identity.session_id,
                    connection_state,
                    compute_staging_dir(&app)?,
                );
                Some(ComputeProviderSchedulingSnapshotV1 {
                    state: runtime.state(),
                    session,
                })
            } else {
                None
            }
        } else {
            None
        }
    };
    let providers = provider_snapshot
        .clone()
        .map(|provider| vec![provider])
        .unwrap_or_default();
    let voice_runtime = creator_voice_runtime_v1(&app, &pack, provider_snapshot.as_ref())?;
    let voice_plan = plan_creator_voice_orchestration_v1(
        &mut store,
        &artifacts,
        &creator.content,
        &voice_runtime,
        &providers,
    )
    .map_err(error_string)?;

    if !voice_plan.all_complete() {
        if voice_plan.burst.scheduled_job_count() > 0 {
            let mut guard = state.compute.lock().map_err(lock_error)?;
            if let Some(runtime) = guard.as_mut() {
                if runtime.state() == ComputeProviderConnectionState::Ready {
                    let _ = dispatch_creator_voice_burst_v1(
                        &mut store,
                        runtime.provider_mut(),
                        &voice_plan,
                    )
                    .map_err(error_string)?;
                }
            }
        }
        drop(store);
        return snapshot_from_active(&state);
    }

    assemble_creator_production_pack_v1(
        &mut store,
        &artifacts,
        &project_id,
        &CreatorProductionPackOptionsV1::default(),
    )
    .map_err(error_string)?;

    drop(store);
    snapshot_from_active(&state)
}

#[tauri::command]
fn production_export_status(
    state: State<'_, DesktopState>,
    project_id: String,
) -> Result<ProductionExportViewV1, String> {
    let data_root = active_data_root(&state)?;
    let store = readable_store(&state)?;
    store.get_project(&project_id).map_err(error_string)?;
    let artifacts = ArtifactStore::new(data_root).map_err(error_string)?;
    production_export_view_v1(&store, &artifacts, &project_id, None, None)
}

#[tauri::command]
fn assemble_production_pack(
    state: State<'_, DesktopState>,
    project_id: String,
) -> Result<ProductionExportViewV1, String> {
    let data_root = active_data_root(&state)?;
    let mut store = writable_store(&state)?;
    store.get_project(&project_id).map_err(error_string)?;
    let artifacts = ArtifactStore::new(data_root).map_err(error_string)?;
    assemble_creator_production_pack_v1(
        &mut store,
        &artifacts,
        &project_id,
        &CreatorProductionPackOptionsV1::default(),
    )
    .map_err(error_string)?;
    production_export_view_v1(&store, &artifacts, &project_id, None, None)
}

#[tauri::command]
fn export_production_pack(
    state: State<'_, DesktopState>,
    project_id: String,
) -> Result<ProductionExportViewV1, String> {
    let data_root = active_data_root(&state)?;
    let mut store = writable_store(&state)?;
    store.get_project(&project_id).map_err(error_string)?;
    let artifacts = ArtifactStore::new(data_root).map_err(error_string)?;
    let assembled = assemble_creator_production_pack_v1(
        &mut store,
        &artifacts,
        &project_id,
        &CreatorProductionPackOptionsV1::default(),
    )
    .map_err(error_string)?;
    let exporter = ProductionPackageExporterV1::default();

    match exporter.export_v1(&mut store, &artifacts, &assembled.production_pack) {
        Ok(outcome) => {
            production_export_view_v1(&store, &artifacts, &project_id, Some(outcome), None)
        }
        Err(error) => {
            let diagnostic = production_export_diagnostic_v1(&error, &assembled.production_pack);
            production_export_view_v1(&store, &artifacts, &project_id, None, Some(diagnostic))
        }
    }
}

#[tauri::command]
fn llmgateway_status(app: AppHandle) -> Result<LlmGatewayStatusView, String> {
    llmgateway_status_for_app(&app)
}

#[tauri::command]
fn save_llmgateway_settings(
    app: AppHandle,
    base_url: String,
    api_key_env: String,
    default_model: String,
) -> Result<LlmGatewayStatusView, String> {
    let path = llmgateway_config_path(&app)?;
    let mut config = if path.exists() {
        LlmGatewayConfig::load(&path).map_err(error_string)?
    } else {
        LlmGatewayConfig::default()
    };

    config.base_url = base_url.trim().to_owned();
    config.api_key_env = api_key_env.trim().to_owned();
    config.default_model = default_model.trim().to_owned();
    config.save(&path).map_err(error_string)?;

    llmgateway_status_for_app(&app)
}

#[tauri::command]
fn compute_provider_status(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<ComputeProviderStatusViewV1, String> {
    let config = load_compute_provider_config(&app)?;
    let guard = state.compute.lock().map_err(lock_error)?;
    Ok(compute_provider_status_view_v1(config, guard.as_ref()))
}

#[tauri::command]
fn connect_compute_provider(
    app: AppHandle,
    state: State<'_, DesktopState>,
    provider_id: String,
    base_url: String,
    bearer_token_env: Option<String>,
) -> Result<ComputeProviderStatusViewV1, String> {
    let config = HttpComputeProviderConfigV1 {
        provider_id: provider_id.trim().to_owned(),
        base_url: base_url.trim().trim_end_matches('/').to_owned(),
        bearer_token_env: bearer_token_env
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        timeout_seconds: 30,
    };
    config.validate_v1().map_err(error_string)?;
    save_compute_provider_config(&app, &config)?;

    let provider = HttpComputeProvider::new(config.clone()).map_err(error_string)?;
    let mut runtime = ComputeProviderRuntime::new(
        provider,
        ComputeProviderLivenessPolicyV1 {
            stale_after_seconds: 30,
            lost_after_seconds: 120,
        },
    )
    .map_err(error_string)?;
    runtime.connect(Utc::now()).map_err(error_string)?;

    let view = compute_provider_status_view_v1(config, Some(&runtime));
    *state.compute.lock().map_err(lock_error)? = Some(runtime);
    Ok(view)
}

#[tauri::command]
fn disconnect_compute_provider(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<ComputeProviderStatusViewV1, String> {
    let config = load_compute_provider_config(&app)?;
    let mut guard = state.compute.lock().map_err(lock_error)?;
    if let Some(runtime) = guard.as_mut() {
        runtime.disconnect().map_err(error_string)?;
    }
    *guard = None;
    Ok(compute_provider_status_view_v1(config, None))
}

#[tauri::command]
fn sync_compute_burst(
    app: AppHandle,
    state: State<'_, DesktopState>,
    project_ids: Vec<String>,
) -> Result<ComputeBurstSyncViewV1, String> {
    let config = load_compute_provider_config(&app)?;
    let data_root = active_data_root(&state)?;
    let staging_dir = compute_staging_dir(&app)?;
    let artifacts = ArtifactStore::new(&data_root).map_err(error_string)?;
    let mut store = writable_store(&state)?;

    let mut guard = state.compute.lock().map_err(lock_error)?;
    let runtime = guard
        .as_mut()
        .ok_or_else(|| "No compute provider is connected.".to_owned())?;
    runtime.heartbeat(Utc::now()).map_err(error_string)?;
    let session = runtime
        .session()
        .cloned()
        .ok_or_else(|| "Connected compute provider has no active session.".to_owned())?;
    let connection_state = runtime.state();
    let reconciliation = reconcile_remote_session_v1(
        &mut store,
        &artifacts,
        runtime.provider_mut(),
        &session.identity.provider_id,
        &session.identity.session_id,
        connection_state,
        staging_dir,
    )
    .map_err(error_string)?;
    let queues = store
        .gpu_workbench_queue_snapshot_v1(&project_ids)
        .map_err(error_string)?;

    Ok(ComputeBurstSyncViewV1 {
        provider: compute_provider_status_view_v1(config, Some(runtime)),
        reconciliation,
        queues,
    })
}

#[tauri::command]
fn gpu_workbench_review(
    state: State<'_, DesktopState>,
    input: GpuWorkbenchPrepareInputV1,
) -> Result<GpuWorkbenchReviewViewV1, String> {
    let store = readable_store(&state)?;
    let week_start = parse_utc(&input.week_start, "GPU weekly budget week_start")?;
    let now = Utc::now();
    let connected_provider = {
        let guard = state.compute.lock().map_err(lock_error)?;
        guard.as_ref().and_then(|runtime| {
            runtime
                .session()
                .cloned()
                .map(|session| ComputeProviderSchedulingSnapshotV1 {
                    state: runtime.state(),
                    session,
                })
        })
    };
    let providers = connected_provider
        .map(|provider| vec![provider])
        .unwrap_or(input.providers);
    let batch = store
        .plan_gpu_batch_v1(
            &GpuBatchPlanRequestV1 {
                project_ids: input.project_ids.clone(),
                preparations: input.preparations,
            },
            &providers,
            &input.running,
        )
        .map_err(error_string)?;
    let workload = store
        .estimate_gpu_batch_workload_v1(&batch)
        .map_err(error_string)?;
    let burst = store
        .plan_gpu_burst_v1(&batch, &providers)
        .map_err(error_string)?;
    let queues = store
        .gpu_workbench_queue_snapshot_v1(&input.project_ids)
        .map_err(error_string)?;

    let ready_provider_ids = batch
        .ready_jobs
        .iter()
        .filter_map(|job| {
            job.eligibility
                .selection
                .as_ref()
                .map(|selection| selection.provider_id.clone())
        })
        .collect::<BTreeSet<_>>();
    let budget = if ready_provider_ids.len() == 1 {
        let provider_id = ready_provider_ids
            .iter()
            .next()
            .expect("one provider id was just counted");
        store
            .assess_gpu_batch_budget_v1(&batch, provider_id, week_start, now)
            .map_err(error_string)?
    } else {
        None
    };

    let startable = batch.is_ready_to_start()
        && burst.blocked.is_empty()
        && burst.preflight_blocked_job_ids.is_empty()
        && burst.scheduled_job_count() == batch.ready_jobs.len();

    Ok(GpuWorkbenchReviewViewV1 {
        batch,
        workload,
        budget,
        burst,
        queues,
        startable,
    })
}

#[tauri::command]
fn set_gpu_weekly_budget(
    state: State<'_, DesktopState>,
    provider_id: String,
    allowance_hours: f64,
) -> Result<(), String> {
    if !allowance_hours.is_finite() || allowance_hours <= 0.0 {
        return Err("Weekly GPU allowance must be greater than zero hours.".to_owned());
    }
    let store = writable_store(&state)?;
    store
        .set_gpu_weekly_budget_v1(&provider_id, allowance_hours * 3600.0, Utc::now())
        .map(|_| ())
        .map_err(error_string)
}

#[tauri::command]
fn start_gpu_burst(
    app: AppHandle,
    state: State<'_, DesktopState>,
    input: GpuBurstStartInputV1,
) -> Result<GpuBurstStartViewV1, String> {
    if input.expected_schedule_hash.trim().is_empty() {
        return Err("Reviewed Burst schedule hash is required.".to_owned());
    }
    if !input.reviewed_batch.is_ready_to_start() {
        return Err(
            "The reviewed GPU batch still contains blocked work. Resolve preflight actions before Burst Mode."
                .to_owned(),
        );
    }

    let mut runtime_guard = state.compute.lock().map_err(lock_error)?;
    let runtime = runtime_guard
        .as_mut()
        .ok_or_else(|| "Connect a compute provider before starting Burst Mode.".to_owned())?;
    if runtime.state() != ComputeProviderConnectionState::Ready {
        return Err(format!(
            "Compute provider must be READY before Burst Mode; found {}.",
            runtime.state().as_str()
        ));
    }
    let connected = runtime
        .session()
        .cloned()
        .ok_or_else(|| "Connected compute provider has no active session.".to_owned())?;
    let providers = vec![ComputeProviderSchedulingSnapshotV1 {
        state: runtime.state(),
        session: connected.clone(),
    }];

    if !input.providers.is_empty()
        && input.providers.iter().any(|provider| {
            provider.session.identity.provider_id != connected.identity.provider_id
                || provider.session.identity.session_id != connected.identity.session_id
        })
    {
        return Err(
            "The connected provider session differs from the reviewed provider snapshot. Prepare the GPU batch again."
                .to_owned(),
        );
    }

    let mut store = writable_store(&state)?;
    let dispatch = dispatch_gpu_burst_v1(
        &mut store,
        runtime.provider_mut(),
        &input.reviewed_batch,
        &providers,
        &input.execution_specs,
        &input.expected_schedule_hash,
    )
    .map_err(error_string)?;

    let data_root = active_data_root(&state)?;
    let artifacts = ArtifactStore::new(&data_root).map_err(error_string)?;
    let connection_state = runtime.state();
    let reconciliation = reconcile_remote_session_v1(
        &mut store,
        &artifacts,
        runtime.provider_mut(),
        &connected.identity.provider_id,
        &connected.identity.session_id,
        connection_state,
        compute_staging_dir(&app)?,
    )
    .map_err(error_string)?;
    let queues = store
        .gpu_workbench_queue_snapshot_v1(&input.reviewed_batch.selected_project_ids)
        .map_err(error_string)?;

    Ok(GpuBurstStartViewV1 {
        dispatch,
        reconciliation,
        queues,
    })
}

#[tauri::command]
fn prepare_device_handoff(state: State<'_, DesktopState>) -> Result<AppSnapshot, String> {
    let active = {
        let mut guard = state.active.lock().map_err(lock_error)?;
        guard
            .take()
            .ok_or_else(|| "No Data Folder is currently open.".to_owned())?
    };

    match active {
        ActiveWorkspace::ReadOnly(workspace) => {
            let data_root = path_text(workspace.data_root());
            let mut guard = state.active.lock().map_err(lock_error)?;
            *guard = Some(ActiveWorkspace::ReadOnly(workspace));
            Err(format!(
                "The workspace at {data_root} is open read-only and cannot prepare a handoff."
            ))
        }
        ActiveWorkspace::Writable(session) => {
            let data_root = path_text(session.workspace().data_root());
            let store = StateStore::open(session.sqlite_path()).map_err(error_string)?;
            let handoff = session.prepare_handoff(&store, 3).map_err(error_string)?;
            Ok(handoff_snapshot(data_root, handoff))
        }
    }
}

fn open_path(
    app: &AppHandle,
    state: &State<'_, DesktopState>,
    data_root: PathBuf,
    device_id: String,
    create_if_missing: bool,
    read_only: bool,
) -> Result<AppSnapshot, String> {
    clean_active_for_switch(state)?;

    let manifest_path = data_root.join(".omnicreator/workspace.json");
    let workspace = if manifest_path.exists() {
        if read_only {
            Workspace::inspect(&data_root).map_err(error_string)?
        } else {
            Workspace::open(&data_root).map_err(error_string)?
        }
    } else if create_if_missing {
        if read_only {
            return Err("A new Data Folder cannot be created in read-only mode.".to_owned());
        }
        Workspace::create(&data_root).map_err(error_string)?
    } else {
        return Ok(AppSnapshot::Unavailable {
            data_root: path_text(&data_root),
            message: "No OmniCreator workspace manifest was found in this folder.".to_owned(),
        });
    };

    let binding = MachineBinding::for_workspace(&workspace, &device_id);
    binding.save(binding_path(app)?).map_err(error_string)?;

    if read_only {
        validate_read_only(&workspace)?;
        {
            let mut guard = state.active.lock().map_err(lock_error)?;
            *guard = Some(ActiveWorkspace::ReadOnly(workspace));
        }
        return snapshot_from_active(state);
    }

    let session = match WorkspaceSession::acquire(workspace, &device_id) {
        Ok(session) => session,
        Err(CoreError::WorkspaceBusy(message)) => {
            return Ok(AppSnapshot::Conflict {
                data_root: path_text(&data_root),
                message,
            });
        }
        Err(error) => return Err(error_string(error)),
    };

    prepare_writable_session(&session)?;
    {
        let mut guard = state.active.lock().map_err(lock_error)?;
        *guard = Some(ActiveWorkspace::Writable(session));
    }
    snapshot_from_active(state)
}

fn prepare_writable_session(session: &WorkspaceSession) -> Result<(), String> {
    let workspace = session.workspace();
    let sqlite_path = workspace.sqlite_path();
    let handoff_path = workspace
        .data_root()
        .join(".omnicreator/handoff/latest.json");

    if sqlite_path.exists() || handoff_path.exists() {
        workspace.recover_if_needed().map_err(error_string)?;
    } else if workspace.manifest().revision > 0 {
        return Err(
            "Workspace state is missing and no verified handoff snapshot is available.".to_owned(),
        );
    }

    let mut store = StateStore::open(&sqlite_path).map_err(error_string)?;
    store.reconcile_interrupted_jobs().map_err(error_string)?;
    Ok(())
}

fn validate_read_only(workspace: &Workspace) -> Result<(), String> {
    let sqlite_path = workspace.sqlite_path();
    if !sqlite_path.exists() {
        return Err("This workspace does not have a local state database yet.".to_owned());
    }
    StateStore::open_read_only(sqlite_path)
        .map(|_| ())
        .map_err(error_string)
}

fn snapshot_from_active(state: &State<'_, DesktopState>) -> Result<AppSnapshot, String> {
    let guard = state.active.lock().map_err(lock_error)?;
    let active = guard
        .as_ref()
        .ok_or_else(|| "No Data Folder is currently open.".to_owned())?;
    let workspace = active.workspace();

    let store = if active.is_read_only() {
        StateStore::open_read_only(workspace.sqlite_path()).map_err(error_string)?
    } else {
        StateStore::open(workspace.sqlite_path()).map_err(error_string)?
    };

    let artifacts = ArtifactStore::new(workspace.data_root()).map_err(error_string)?;
    let projects = store
        .list_projects()
        .map_err(error_string)?
        .into_iter()
        .map(|project| {
            let status = store
                .derive_project_status(&project.id)
                .map_err(error_string)?;
            let jobs = store.list_project_jobs(&project.id).map_err(error_string)?;
            let steps = store
                .list_project_steps(&project.id)
                .map_err(error_string)?;
            let board = project_board_projection_v1(status, &jobs, &steps);
            let run = project.studio_pack.as_ref().and_then(|_| {
                derive_creator_run_coordinator_v1(&store, &artifacts, &project.id).ok()
            });
            Ok(ProjectView {
                project,
                status,
                board,
                steps,
                run,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(AppSnapshot::Ready {
        workspace: WorkspaceView {
            data_root: path_text(workspace.data_root()),
            workspace_id: workspace.manifest().workspace_id.clone(),
            revision: workspace.manifest().revision,
            last_clean_shutdown: workspace.manifest().last_clean_shutdown,
            last_writer_device: workspace.manifest().last_writer_device.clone(),
            read_only: active.is_read_only(),
        },
        projects,
    })
}

fn studio_pack_catalog_path_v1(data_root: &Path) -> PathBuf {
    data_root.join(".omnicreator/studio-pack-catalog.json")
}

fn load_studio_pack_catalog_v1(data_root: &Path) -> Result<PortableStudioPackCatalogV1, String> {
    let built_in = initial_studio_pack_catalog_v1().map_err(error_string)?;
    let path = studio_pack_catalog_path_v1(data_root);
    if !path.exists() {
        return Ok(built_in);
    }

    let raw = fs::read_to_string(&path)
        .map_err(|error| format!("Cannot read portable Studio Pack catalog: {error}"))?;
    let stored = PortableStudioPackCatalogV1::from_json_v1(&raw).map_err(error_string)?;
    let built_in_ids = built_in
        .packs
        .iter()
        .map(|pack| pack.id.clone())
        .collect::<BTreeSet<_>>();
    let mut packs = built_in.packs;
    packs.extend(
        stored
            .packs
            .into_iter()
            .filter(|pack| !built_in_ids.contains(pack.id.as_str())),
    );
    PortableStudioPackCatalogV1::from_packs_v1(packs).map_err(error_string)
}

fn save_studio_pack_catalog_v1(
    data_root: &Path,
    catalog: &PortableStudioPackCatalogV1,
) -> Result<(), String> {
    let path = studio_pack_catalog_path_v1(data_root);
    let parent = path
        .parent()
        .ok_or_else(|| "Studio Pack catalog path has no parent.".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Cannot create Studio Pack catalog directory: {error}"))?;
    let temporary = parent.join("studio-pack-catalog.json.tmp");
    let json = catalog.canonical_json_v1().map_err(error_string)?;
    fs::write(&temporary, json)
        .map_err(|error| format!("Cannot write portable Studio Pack catalog: {error}"))?;
    fs::rename(&temporary, &path)
        .map_err(|error| format!("Cannot commit portable Studio Pack catalog: {error}"))
}

fn plugin_built_in_roots_v1(app: &AppHandle) -> Vec<PathBuf> {
    let mut roots = BTreeSet::new();
    if let Ok(current) = env::current_dir() {
        roots.insert(current.join("plugins"));
        roots.insert(current.join("../plugins"));
        roots.insert(current.join("../../plugins"));
    }
    if let Ok(resources) = app.path().resource_dir() {
        roots.insert(resources.join("plugins"));
    }
    roots.into_iter().collect()
}

fn plugin_user_root_v1(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Cannot resolve app data directory: {error}"))?;
    Ok(app_data.join("plugins"))
}

fn plugin_lifecycle_path_v1(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("Cannot resolve app config directory: {error}"))?;
    fs::create_dir_all(&config_dir)
        .map_err(|error| format!("Cannot create app config directory: {error}"))?;
    Ok(config_dir.join("plugin-lifecycle.json"))
}

fn load_plugin_lifecycle_v1(app: &AppHandle) -> Result<PluginLifecycleStateV1, String> {
    PluginLifecycleStateV1::load_v1(plugin_lifecycle_path_v1(app)?).map_err(error_string)
}

fn plugin_inventory_report_v1(app: &AppHandle) -> Result<PluginInventoryReportV1, String> {
    let lifecycle = load_plugin_lifecycle_v1(app)?;
    let built_in_roots = plugin_built_in_roots_v1(app);
    let user_root = plugin_user_root_v1(app)?;
    Ok(scan_plugin_inventory_v1(
        &built_in_roots,
        &[user_root],
        &lifecycle,
    ))
}

fn plugin_impact_context_v1(
    state: &State<'_, DesktopState>,
) -> Result<(PortableStudioPackCatalogV1, Vec<Project>), String> {
    let active = {
        let guard = state.active.lock().map_err(lock_error)?;
        guard.as_ref().map(|active| {
            (
                active.workspace().data_root().to_path_buf(),
                active.workspace().sqlite_path(),
                active.is_read_only(),
            )
        })
    };

    let Some((data_root, sqlite_path, read_only)) = active else {
        return Ok((
            initial_studio_pack_catalog_v1().map_err(error_string)?,
            Vec::new(),
        ));
    };

    let catalog = load_studio_pack_catalog_v1(&data_root)?;
    let store = if read_only {
        StateStore::open_read_only(sqlite_path).map_err(error_string)?
    } else {
        StateStore::open(sqlite_path).map_err(error_string)?
    };
    let projects = store.list_projects().map_err(error_string)?;
    Ok((catalog, projects))
}

fn studio_pack_plugin_registry_v1(app: &AppHandle) -> Result<PluginRegistry, String> {
    Ok(plugin_inventory_report_v1(app)?.registry)
}

fn studio_pack_runtime_snapshot_v1(
    app: &AppHandle,
    registry: &PluginRegistry,
) -> Result<StudioPackRuntimeSnapshotV1, String> {
    let lifecycle = load_plugin_lifecycle_v1(app)?;
    let mut runtime = StudioPackRuntimeSnapshotV1::default();
    for plugin in registry.plugins() {
        if !lifecycle.is_enabled_v1(&plugin.manifest.id) {
            runtime.set_v1(
                plugin.manifest.id.clone(),
                PluginRuntimeReadinessV1::Unavailable {
                    reason_code: "PLUGIN_DISABLED".to_owned(),
                },
            );
            continue;
        }

        let report = load_plugin_settings_ui(plugin);
        if !report.diagnostics.is_empty() {
            runtime.set_v1(
                plugin.manifest.id.clone(),
                PluginRuntimeReadinessV1::Unavailable {
                    reason_code: "PLUGIN_SETTINGS_INVALID".to_owned(),
                },
            );
            continue;
        }

        let missing_credential = report.ui.as_ref().and_then(|ui| {
            ui.fields
                .iter()
                .filter(|field| field.key.ends_with("_env"))
                .find_map(|field| {
                    let name = field.default.as_ref()?.as_str()?.trim();
                    if name.is_empty() {
                        return None;
                    }
                    let present = env::var(name)
                        .map(|value| !value.trim().is_empty())
                        .unwrap_or(false);
                    (!present).then(|| name.to_owned())
                })
        });

        let readiness = match missing_credential {
            Some(name) => PluginRuntimeReadinessV1::SetupRequired {
                reason_code: format!("CREDENTIAL_ENV_MISSING:{name}"),
            },
            None => PluginRuntimeReadinessV1::Ready,
        };
        runtime.set_v1(plugin.manifest.id.clone(), readiness);
    }
    Ok(runtime)
}

fn studio_pack_catalog_view_v1(
    app: &AppHandle,
    state: &State<'_, DesktopState>,
) -> Result<StudioPackCatalogDesktopViewV1, String> {
    let data_root = active_data_root(state)?;
    let catalog = load_studio_pack_catalog_v1(&data_root)?;
    let built_in = initial_studio_pack_catalog_v1().map_err(error_string)?;
    let built_in_ids = built_in
        .packs
        .iter()
        .map(|pack| pack.id.clone())
        .collect::<BTreeSet<_>>();
    let registry = studio_pack_plugin_registry_v1(app)?;
    let runtime = studio_pack_runtime_snapshot_v1(app, &registry)?;

    let mut packs = Vec::new();
    for definition in catalog.list_definitions_v1().map_err(error_string)? {
        let effective = catalog.resolve_v1(&definition.id).map_err(error_string)?;
        let availability = catalog
            .evaluate_availability_v1(&definition.id, &registry, &runtime)
            .map_err(error_string)?;
        let pack = build_studio_pack_ux_view_v1(definition, &effective, &availability)
            .map_err(error_string)?;
        packs.push(StudioPackCatalogItemDesktopViewV1 {
            custom: !built_in_ids.contains(definition.id.as_str()),
            pack,
        });
    }
    Ok(StudioPackCatalogDesktopViewV1 { packs })
}

fn validate_desktop_studio_pack_overrides_v1(
    catalog: &PortableStudioPackCatalogV1,
    base_pack_id: &str,
    overrides: &StudioPackOverridesV1,
) -> Result<(), String> {
    let base = catalog.resolve_v1(base_pack_id).map_err(error_string)?;

    if !overrides.routes.is_empty()
        || !overrides.remove_routes.is_empty()
        || !overrides.remove_presets.is_empty()
        || !overrides.remove_quality_thresholds.is_empty()
    {
        return Err(
            "Desktop creator overrides may change curated presets, automation and existing quality thresholds only; routing remains an Advanced projection over canonical Studio Pack routes."
                .to_owned(),
        );
    }

    for (key, value) in &overrides.presets {
        let mut allowed = BTreeSet::new();
        for definition in catalog.list_definitions_v1().map_err(error_string)? {
            let effective = catalog.resolve_v1(&definition.id).map_err(error_string)?;
            if let Some(candidate) = effective.config.presets.get(key) {
                allowed.insert(candidate.clone());
            }
        }
        if !allowed.contains(value) {
            return Err(format!(
                "Preset override {key}={value} is not one of the capability-compatible catalog presets."
            ));
        }
    }

    for key in overrides.quality_thresholds.keys() {
        if !base.config.quality_thresholds.contains_key(key) {
            return Err(format!(
                "Quality override '{key}' is not defined by the selected Studio Pack."
            ));
        }
    }

    let probe = StudioPackV1 {
        schema: STUDIO_PACK_SCHEMA_V1.to_owned(),
        schema_version: STUDIO_PACK_VERSION_V1,
        id: "desktop-override-validation".to_owned(),
        name: "Desktop Override Validation".to_owned(),
        extends: Some(base_pack_id.to_owned()),
        overrides: overrides.clone(),
    };
    probe.validate_v1().map_err(error_string)?;
    let mut packs = catalog.packs.clone();
    packs.retain(|pack| pack.id != probe.id);
    packs.push(probe);
    PortableStudioPackCatalogV1::from_packs_v1(packs)
        .map(|_| ())
        .map_err(error_string)
}

fn studio_review_center_view_v1(
    app: &AppHandle,
    state: &State<'_, DesktopState>,
) -> Result<StudioReviewCenterV1, String> {
    let data_root = active_data_root(state)?;
    let store = readable_store(state)?;
    let catalog = load_studio_pack_catalog_v1(&data_root)?;
    let registry = studio_pack_plugin_registry_v1(app)?;
    let runtime = studio_pack_runtime_snapshot_v1(app, &registry)?;
    let mut projects = Vec::new();

    for project in store.list_projects().map_err(error_string)? {
        let jobs = store
            .list_project_jobs(&project.id)
            .map_err(error_string)?
            .into_iter()
            .map(|job| {
                let attempts = store.list_attempts(&job.job_id).map_err(error_string)?;
                Ok(StudioJobReviewSnapshotV1 { job, attempts })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let steps = store
            .list_project_steps(&project.id)
            .map_err(error_string)?;
        let availability = match project.studio_pack.as_deref() {
            Some(pack_id) => Some(
                catalog
                    .evaluate_availability_v1(pack_id, &registry, &runtime)
                    .map_err(error_string)?,
            ),
            None => None,
        };
        projects.push((project, jobs, steps, availability));
    }

    Ok(build_studio_review_center_v1(&projects))
}

fn compute_provider_status_view_v1(
    config: HttpComputeProviderConfigV1,
    runtime: Option<&ComputeProviderRuntime<HttpComputeProvider>>,
) -> ComputeProviderStatusViewV1 {
    let credential_present = config
        .bearer_token_env
        .as_deref()
        .map(|name| env::var(name).is_ok_and(|value| !value.trim().is_empty()))
        .unwrap_or(true);
    let (state, session_id, capabilities, message) = match runtime {
        Some(runtime) => {
            let session = runtime.session();
            (
                runtime.state().as_str().to_owned(),
                session.map(|value| value.identity.session_id.clone()),
                session.map(|value| value.capabilities.clone()),
                if runtime.state() == ComputeProviderConnectionState::Ready {
                    "Compute worker is READY for reviewed Burst work.".to_owned()
                } else {
                    format!("Compute worker is {}.", runtime.state().as_str())
                },
            )
        }
        None => (
            ComputeProviderConnectionState::Disconnected
                .as_str()
                .to_owned(),
            None,
            None,
            if config.bearer_token_env.is_some() && !credential_present {
                "Compute worker is disconnected; configured credential environment variable is unavailable."
                    .to_owned()
            } else {
                "Compute worker is disconnected.".to_owned()
            },
        ),
    };

    ComputeProviderStatusViewV1 {
        state,
        provider_id: config.provider_id,
        base_url: config.base_url,
        bearer_token_env: config.bearer_token_env,
        credential_present,
        session_id,
        capabilities,
        message,
    }
}

fn default_compute_provider_config() -> HttpComputeProviderConfigV1 {
    HttpComputeProviderConfigV1 {
        provider_id: "remote-gpu".to_owned(),
        base_url: "http://127.0.0.1:8787".to_owned(),
        bearer_token_env: Some("OMNICREATOR_COMPUTE_TOKEN".to_owned()),
        timeout_seconds: 30,
    }
}

fn load_compute_provider_config(app: &AppHandle) -> Result<HttpComputeProviderConfigV1, String> {
    let path = compute_provider_config_path(app)?;
    if !path.exists() {
        return Ok(default_compute_provider_config());
    }
    let bytes =
        fs::read(&path).map_err(|error| format!("Cannot read compute provider config: {error}"))?;
    let config: HttpComputeProviderConfigV1 = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid compute provider config JSON: {error}"))?;
    config.validate_v1().map_err(error_string)?;
    Ok(config)
}

fn save_compute_provider_config(
    app: &AppHandle,
    config: &HttpComputeProviderConfigV1,
) -> Result<(), String> {
    config.validate_v1().map_err(error_string)?;
    let path = compute_provider_config_path(app)?;
    let bytes = serde_json::to_vec_pretty(config)
        .map_err(|error| format!("Cannot encode compute provider config: {error}"))?;
    fs::write(path, bytes).map_err(|error| format!("Cannot save compute provider config: {error}"))
}

fn compute_provider_config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("Cannot resolve app config directory: {error}"))?;
    fs::create_dir_all(&config_dir)
        .map_err(|error| format!("Cannot create app config directory: {error}"))?;
    Ok(config_dir.join("compute-provider.json"))
}

fn compute_staging_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("Cannot resolve app cache directory: {error}"))?;
    let staging = cache_dir.join("compute-staging");
    fs::create_dir_all(&staging)
        .map_err(|error| format!("Cannot create compute staging directory: {error}"))?;
    Ok(staging)
}

fn active_data_root(state: &State<'_, DesktopState>) -> Result<PathBuf, String> {
    let guard = state.active.lock().map_err(lock_error)?;
    let active = guard
        .as_ref()
        .ok_or_else(|| "No Data Folder is currently open.".to_owned())?;
    Ok(active.workspace().data_root().to_path_buf())
}

fn readable_store(state: &State<'_, DesktopState>) -> Result<StateStore, String> {
    let guard = state.active.lock().map_err(lock_error)?;
    let active = guard
        .as_ref()
        .ok_or_else(|| "No Data Folder is currently open.".to_owned())?;
    if active.is_read_only() {
        StateStore::open_read_only(active.workspace().sqlite_path()).map_err(error_string)
    } else {
        StateStore::open(active.workspace().sqlite_path()).map_err(error_string)
    }
}

fn writable_store(state: &State<'_, DesktopState>) -> Result<StateStore, String> {
    let sqlite_path = {
        let mut guard = state.active.lock().map_err(lock_error)?;
        let active = guard
            .as_mut()
            .ok_or_else(|| "No Data Folder is currently open.".to_owned())?;
        active.refresh_lease()?;
        match active {
            ActiveWorkspace::Writable(session) => session.sqlite_path(),
            ActiveWorkspace::ReadOnly(_) => {
                return Err("This workspace is open read-only.".to_owned());
            }
        }
    };
    StateStore::open(sqlite_path).map_err(error_string)
}

fn production_export_view_v1(
    store: &StateStore,
    artifacts: &ArtifactStore,
    project_id: &str,
    outcome: Option<ProductionPackageExportOutcomeV1>,
    diagnostic: Option<ProductionExportDiagnosticViewV1>,
) -> Result<ProductionExportViewV1, String> {
    let history = store
        .production_export_history_v1(project_id)
        .map_err(error_string)?;
    let assembled = load_latest_creator_production_pack_v1(store, artifacts, project_id)
        .map_err(error_string)?;
    let last_pack =
        latest_portable_production_pack_v1(artifacts, project_id, &history).or_else(|| {
            assembled
                .as_ref()
                .map(|outcome| outcome.production_pack.clone())
        });
    let state = if let Some(outcome) = outcome.as_ref() {
        if outcome.cache_hit {
            "cached".to_owned()
        } else {
            "succeeded".to_owned()
        }
    } else if diagnostic.is_some() {
        history
            .first()
            .map(|entry| entry.job.status.as_str().to_ascii_lowercase())
            .unwrap_or_else(|| "failed".to_owned())
    } else if let Some(entry) = history.first() {
        entry.job.status.as_str().to_ascii_lowercase()
    } else if assembled.is_some() {
        "assembled".to_owned()
    } else {
        "not_assembled".to_owned()
    };

    Ok(ProductionExportViewV1 {
        project_id: project_id.to_owned(),
        state,
        outcome,
        history,
        last_pack,
        diagnostic,
    })
}

fn latest_portable_production_pack_v1(
    artifacts: &ArtifactStore,
    project_id: &str,
    history: &[ProductionExportHistoryEntryV1],
) -> Option<ProductionPackV1> {
    for entry in history {
        for artifact in &entry.artifacts {
            if artifact.artifact_type != "production-pack" {
                continue;
            }
            if !matches!(artifacts.verify_artifact(artifact), Ok(true)) {
                continue;
            }
            let Ok(path) = artifacts.resolve_artifact_path(artifact) else {
                continue;
            };
            let Ok(bytes) = fs::read(path) else {
                continue;
            };
            let Ok(pack) = serde_json::from_slice::<ProductionPackV1>(&bytes) else {
                continue;
            };
            if pack.project_id == project_id {
                return Some(pack);
            }
        }
    }
    None
}

fn production_export_diagnostic_v1(
    error: &CoreError,
    production_pack: &ProductionPackV1,
) -> ProductionExportDiagnosticViewV1 {
    match error {
        CoreError::ArtifactNotFound(artifact_id)
        | CoreError::ExportArtifactFileMissing { artifact_id, .. } => {
            ProductionExportDiagnosticViewV1 {
                kind: "missing_artifact".to_owned(),
                artifact_id: Some(artifact_id.clone()),
                logical_uri: production_pack_logical_uri_v1(production_pack, artifact_id),
                message: "A source artifact required by this Production Pack is missing at the current Data Root binding.".to_owned(),
                action: "Restore or relink the Data Folder/source artifact, then Regenerate Production Pack.".to_owned(),
            }
        }
        CoreError::ArtifactHashMismatch(artifact_id) => ProductionExportDiagnosticViewV1 {
            kind: "artifact_changed".to_owned(),
            artifact_id: Some(artifact_id.clone()),
            logical_uri: production_pack_logical_uri_v1(production_pack, artifact_id),
            message: "A source artifact no longer matches its canonical SHA256.".to_owned(),
            action: "Restore the expected artifact or promote the changed source canonically, then regenerate.".to_owned(),
        },
        CoreError::ExportArtifactUriMismatch { artifact_id, .. }
        | CoreError::ExportArtifactProjectMismatch { artifact_id, .. } => {
            ProductionExportDiagnosticViewV1 {
                kind: "invalid_production_pack".to_owned(),
                artifact_id: Some(artifact_id.clone()),
                logical_uri: production_pack_logical_uri_v1(production_pack, artifact_id),
                message: error.to_string(),
                action: "Refresh the canonical ProductionPack input before exporting again.".to_owned(),
            }
        }
        CoreError::InvalidContract(message) => ProductionExportDiagnosticViewV1 {
            kind: "invalid_production_pack".to_owned(),
            artifact_id: None,
            logical_uri: None,
            message: format!("Production Pack validation failed: {message}"),
            action: "Correct the portable ProductionPack input and export again.".to_owned(),
        },
        _ => ProductionExportDiagnosticViewV1 {
            kind: "export_failure".to_owned(),
            artifact_id: None,
            logical_uri: None,
            message: "Production export failed before the package could be committed successfully.".to_owned(),
            action: "Review canonical Job/Attempt status, resolve the dependency, then retry.".to_owned(),
        },
    }
}

fn production_pack_logical_uri_v1(
    production_pack: &ProductionPackV1,
    artifact_id: &str,
) -> Option<String> {
    production_pack
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.artifact_id == artifact_id)
        .map(|clip| clip.uri.to_string())
}

fn parse_utc(raw: &str, label: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(raw)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| format!("{label} must be RFC3339: {error}"))
}

fn with_writable_store(
    state: &State<'_, DesktopState>,
    operation: impl FnOnce(&StateStore) -> omnicreator_core::Result<()>,
) -> Result<(), String> {
    let sqlite_path = {
        let mut guard = state.active.lock().map_err(lock_error)?;
        let active = guard
            .as_mut()
            .ok_or_else(|| "No Data Folder is currently open.".to_owned())?;
        active.refresh_lease()?;
        match active {
            ActiveWorkspace::Writable(session) => session.sqlite_path(),
            ActiveWorkspace::ReadOnly(_) => {
                return Err("This workspace is open read-only.".to_owned());
            }
        }
    };

    let store = StateStore::open(sqlite_path).map_err(error_string)?;
    operation(&store).map_err(error_string)
}

fn llmgateway_status_for_app(app: &AppHandle) -> Result<LlmGatewayStatusView, String> {
    let config = load_llmgateway_config(app)?;
    let credential_present = env::var(&config.api_key_env)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let client = LlmGatewayClient::new(config.clone()).map_err(error_string)?;

    let health = match client.health() {
        Ok(health) => health,
        Err(error) => {
            return Ok(LlmGatewayStatusView {
                state: LlmGatewayConnectionState::Offline,
                base_url: config.base_url.clone(),
                api_key_env: config.api_key_env.clone(),
                default_model: config.default_model.clone(),
                credential_present,
                health_status: None,
                gateway_default_model: None,
                models: Vec::new(),
                message: format!(
                    "LLMGateway is not reachable at {}. Start LLMGateway or update the endpoint, then refresh. ({error})",
                    config.base_url
                ),
            });
        }
    };

    if !credential_present {
        return Ok(LlmGatewayStatusView {
            state: LlmGatewayConnectionState::NeedsApiKey,
            base_url: config.base_url.clone(),
            api_key_env: config.api_key_env.clone(),
            default_model: config.default_model.clone(),
            credential_present: false,
            health_status: Some(health.status),
            gateway_default_model: health.default_model,
            models: Vec::new(),
            message: format!(
                "Gateway is reachable. Set the {} environment variable on this machine, then refresh. The secret is never stored in the portable Data Folder.",
                config.api_key_env
            ),
        });
    }

    match client.models() {
        Ok(mut models) => {
            sort_llmgateway_models(&mut models);
            let models = models
                .into_iter()
                .map(llmgateway_model_view)
                .collect::<Vec<_>>();
            Ok(LlmGatewayStatusView {
                state: LlmGatewayConnectionState::Ready,
                base_url: config.base_url,
                api_key_env: config.api_key_env,
                default_model: config.default_model,
                credential_present: true,
                health_status: Some(health.status),
                gateway_default_model: health.default_model,
                message: format!(
                    "Connected. {} models discovered; LLMGateway virtual models are listed first.",
                    models.len()
                ),
                models,
            })
        }
        Err(error) => Ok(LlmGatewayStatusView {
            state: LlmGatewayConnectionState::Degraded,
            base_url: config.base_url,
            api_key_env: config.api_key_env,
            default_model: config.default_model,
            credential_present: true,
            health_status: Some(health.status),
            gateway_default_model: health.default_model,
            models: Vec::new(),
            message: format!(
                "Gateway health is reachable, but authenticated model discovery failed. Check the API key and gateway configuration. ({error})"
            ),
        }),
    }
}

fn load_llmgateway_config(app: &AppHandle) -> Result<LlmGatewayConfig, String> {
    let path = llmgateway_config_path(app)?;
    if path.exists() {
        LlmGatewayConfig::load(path).map_err(error_string)
    } else {
        Ok(LlmGatewayConfig::default())
    }
}

fn llmgateway_config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("Cannot resolve app config directory: {error}"))?;
    fs::create_dir_all(&config_dir)
        .map_err(|error| format!("Cannot create app config directory: {error}"))?;
    Ok(config_dir.join("llmgateway.json"))
}

fn sort_llmgateway_models(models: &mut [LlmGatewayModel]) {
    models.sort_by(|left, right| {
        right
            .is_virtual()
            .cmp(&left.is_virtual())
            .then_with(|| left.id.to_lowercase().cmp(&right.id.to_lowercase()))
    });
}

fn llmgateway_model_view(model: LlmGatewayModel) -> LlmGatewayModelView {
    let display_name = model
        .llmgateway
        .as_ref()
        .and_then(|metadata| metadata.display_name.clone())
        .unwrap_or_else(|| model.id.clone());
    let is_virtual = model.is_virtual();

    LlmGatewayModelView {
        id: model.id,
        display_name,
        is_virtual,
    }
}

fn local_device_id(app: &AppHandle) -> Result<String, String> {
    let path = binding_path(app)?;
    if path.exists() {
        if let Ok(binding) = MachineBinding::load(&path) {
            return Ok(binding.device_id);
        }
    }
    Ok(format!("device_{}", Uuid::new_v4().simple()))
}

fn binding_path(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("Cannot resolve app config directory: {error}"))?;
    fs::create_dir_all(&config_dir)
        .map_err(|error| format!("Cannot create app config directory: {error}"))?;
    Ok(config_dir.join("machine-binding.json"))
}

fn absolute_path(raw: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err("Data Folder path must be absolute.".to_owned());
    }
    Ok(path.to_path_buf())
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn error_string(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> String {
    format!("Desktop state lock was poisoned: {error}")
}

fn handoff_snapshot(data_root: String, handoff: HandoffManifest) -> AppSnapshot {
    AppSnapshot::HandoffReady {
        data_root,
        revision: handoff.revision,
        snapshot_sha256: handoff.snapshot_sha256,
    }
}

fn clean_active_for_switch(state: &State<'_, DesktopState>) -> Result<(), String> {
    let active = {
        let mut guard = state.active.lock().map_err(lock_error)?;
        guard.take()
    };

    let Some(active) = active else {
        return Ok(());
    };

    match active {
        ActiveWorkspace::ReadOnly(_) => Ok(()),
        ActiveWorkspace::Writable(session) => {
            let store = StateStore::open(session.sqlite_path()).map_err(error_string)?;
            session.prepare_handoff(&store, 3).map_err(error_string)?;
            Ok(())
        }
    }
}

fn clean_shutdown(state: &DesktopState) {
    let active = match state.active.lock() {
        Ok(mut guard) => guard.take(),
        Err(_) => None,
    };

    let Some(ActiveWorkspace::Writable(session)) = active else {
        return;
    };

    if let Ok(store) = StateStore::open(session.sqlite_path()) {
        let _ = session.prepare_handoff(&store, 3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnicreator_core::{
        LogicalUri, TimelineClipV1, TimelineFrameRateV1, TimelineTrackRoleV1, TimelineTrackV1,
        PRODUCTION_PACK_SCHEMA_V1, PRODUCTION_PACK_VERSION_V1,
    };

    #[test]
    fn missing_artifact_diagnostic_exposes_portable_identity_without_machine_path() {
        let pack = ProductionPackV1 {
            schema: PRODUCTION_PACK_SCHEMA_V1.to_owned(),
            version: PRODUCTION_PACK_VERSION_V1,
            project_id: "project-1".to_owned(),
            title: "Diagnostic".to_owned(),
            frame_rate: TimelineFrameRateV1 {
                numerator: 24,
                denominator: 1,
            },
            tracks: vec![TimelineTrackV1 {
                role: TimelineTrackRoleV1::VideoPrimary,
                clips: vec![TimelineClipV1 {
                    clip_id: "clip-1".to_owned(),
                    artifact_id: "artifact-1".to_owned(),
                    uri: LogicalUri::parse("project://video/SC01.mp4").unwrap(),
                    timeline_start_ms: 0,
                    source_start_ms: 0,
                    duration_ms: 1_000,
                    label: None,
                }],
            }],
            subtitles: Vec::new(),
            markers: Vec::new(),
        };
        let error = CoreError::ExportArtifactFileMissing {
            artifact_id: "artifact-1".to_owned(),
            path: PathBuf::from("/Users/alice/private/source.mp4"),
        };

        let diagnostic = production_export_diagnostic_v1(&error, &pack);

        assert_eq!(diagnostic.kind, "missing_artifact");
        assert_eq!(diagnostic.artifact_id.as_deref(), Some("artifact-1"));
        assert_eq!(
            diagnostic.logical_uri.as_deref(),
            Some("project://video/SC01.mp4")
        );
        assert!(!diagnostic.message.contains("/Users/alice"));
        assert!(!diagnostic.action.contains("/Users/alice"));
    }
}

fn main() {
    let app = tauri::Builder::default()
        .manage(DesktopState::default())
        .invoke_handler(tauri::generate_handler![
            pick_data_root,
            pick_plugin_folder,
            bootstrap,
            create_data_root,
            use_existing_data_root,
            open_read_only,
            check_again,
            heartbeat,
            list_projects,
            create_project,
            create_project_from_studio_pack,
            update_project_studio_pack,
            plugin_inventory,
            set_plugin_enabled,
            install_plugin_from_folder,
            uninstall_plugin,
            inspect_plugin_update,
            apply_plugin_update,
            plugin_mutation_impact,
            studio_pack_catalog,
            asset_library,
            add_asset_tag,
            remove_asset_tag,
            review_center,
            retry_review_job,
            start_creator_production,
            rename_project,
            delete_project,
            production_export_status,
            assemble_production_pack,
            export_production_pack,
            llmgateway_status,
            save_llmgateway_settings,
            compute_provider_status,
            connect_compute_provider,
            disconnect_compute_provider,
            sync_compute_burst,
            gpu_workbench_review,
            set_gpu_weekly_budget,
            start_gpu_burst,
            prepare_device_handoff
        ])
        .build(tauri::generate_context!())
        .expect("failed to build OmniCreator desktop");

    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            let state = app_handle.state::<DesktopState>();
            clean_shutdown(&state);
        }
    });
}

#[cfg(test)]
mod desktop_tests {
    use super::*;

    #[test]
    fn compute_provider_defaults_keep_secrets_out_of_machine_config() {
        let config = default_compute_provider_config();
        config.validate_v1().unwrap();
        let json = serde_json::to_string(&config).unwrap();

        assert_eq!(config.provider_id, "remote-gpu");
        assert_eq!(
            config.bearer_token_env.as_deref(),
            Some("OMNICREATOR_COMPUTE_TOKEN")
        );
        assert!(json.contains("OMNICREATOR_COMPUTE_TOKEN"));
        assert!(!json.contains("Bearer "));
        assert!(!json.to_lowercase().contains("kaggle"));
    }

    #[test]
    fn gpu_week_start_parser_requires_explicit_rfc3339_time() {
        assert!(parse_utc("2026-09-05T00:00:00Z", "week_start").is_ok());
        assert!(parse_utc("2026-09-05", "week_start").is_err());
    }
}
