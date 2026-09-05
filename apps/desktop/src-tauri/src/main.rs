use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use chrono::{DateTime, Utc};
use omnicreator_core::{
    dispatch_gpu_burst_v1, reconcile_remote_session_v1, ArtifactStore,
    ComputeProviderConnectionState, ComputeProviderLivenessPolicyV1, ComputeProviderRuntime,
    ComputeProviderSchedulingSnapshotV1, ComputeRunningAssignmentV1, Error as CoreError,
    GpuBatchBudgetOverviewV1, GpuBatchPlanRequestV1, GpuBatchPlanV1, GpuBurstDispatchSummaryV1,
    GpuBurstPlanV1, GpuJobPreparationV1, GpuWorkbenchQueueSnapshotV1, HandoffManifest,
    HttpComputeProvider, HttpComputeProviderConfigV1, LlmGatewayClient, LlmGatewayConfig,
    LlmGatewayModel, MachineBinding, ProductionExportHistoryEntryV1, ProductionPackV1,
    ProductionPackageExportOutcomeV1, ProductionPackageExporterV1, Project, ProjectDisplayStatus,
    RemoteComputeJobSpecV1, RemoteReconciliationSummaryV1, RuntimeWorkloadEstimateV1, StateStore,
    Workspace, WorkspaceSession,
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
fn export_production_pack(
    state: State<'_, DesktopState>,
    production_pack: ProductionPackV1,
) -> Result<ProductionExportViewV1, String> {
    let project_id = production_pack.project_id.clone();
    let data_root = active_data_root(&state)?;
    let mut store = writable_store(&state)?;
    store.get_project(&project_id).map_err(error_string)?;
    let artifacts = ArtifactStore::new(data_root).map_err(error_string)?;
    let exporter = ProductionPackageExporterV1::default();

    match exporter.export_v1(&mut store, &artifacts, &production_pack) {
        Ok(outcome) => {
            production_export_view_v1(&store, &artifacts, &project_id, Some(outcome), None)
        }
        Err(error) => {
            let diagnostic = production_export_diagnostic_v1(&error, &production_pack);
            production_export_view_v1(
                &store,
                &artifacts,
                &project_id,
                None,
                Some(diagnostic),
            )
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

    let projects = store
        .list_projects()
        .map_err(error_string)?
        .into_iter()
        .map(|project| {
            let status = store
                .derive_project_status(&project.id)
                .unwrap_or(ProjectDisplayStatus::NeedsReview);
            ProjectView { project, status }
        })
        .collect();

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
    let last_pack = latest_portable_production_pack_v1(artifacts, project_id, &history);
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
    } else {
        history
            .first()
            .map(|entry| entry.job.status.as_str().to_ascii_lowercase())
            .unwrap_or_else(|| "not_exported".to_owned())
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
            bootstrap,
            create_data_root,
            use_existing_data_root,
            open_read_only,
            check_again,
            heartbeat,
            list_projects,
            create_project,
            rename_project,
            delete_project,
            production_export_status,
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
