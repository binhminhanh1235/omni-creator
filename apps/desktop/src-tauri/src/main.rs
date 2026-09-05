use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use chrono::{DateTime, Utc};
use omnicreator_core::{
    ComputeProviderSchedulingSnapshotV1, ComputeRunningAssignmentV1, Error as CoreError,
    GpuBatchBudgetOverviewV1, GpuBatchPlanRequestV1, GpuBatchPlanV1, GpuBurstPlanV1,
    GpuJobPreparationV1, GpuWorkbenchQueueSnapshotV1, HandoffManifest, LlmGatewayClient,
    LlmGatewayConfig, LlmGatewayModel, MachineBinding, Project, ProjectDisplayStatus,
    RuntimeWorkloadEstimateV1, StateStore, Workspace, WorkspaceSession,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

#[derive(Default)]
struct DesktopState {
    active: Mutex<Option<ActiveWorkspace>>,
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
    expected_schedule_hash: String,
}

#[derive(Debug, Serialize)]
struct GpuBurstStartViewV1 {
    burst: GpuBurstPlanV1,
    queues: GpuWorkbenchQueueSnapshotV1,
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
fn gpu_workbench_review(
    state: State<'_, DesktopState>,
    input: GpuWorkbenchPrepareInputV1,
) -> Result<GpuWorkbenchReviewViewV1, String> {
    let store = readable_store(&state)?;
    let week_start = parse_utc(&input.week_start, "GPU weekly budget week_start")?;
    let now = Utc::now();
    let batch = store
        .plan_gpu_batch_v1(
            &GpuBatchPlanRequestV1 {
                project_ids: input.project_ids.clone(),
                preparations: input.preparations,
            },
            &input.providers,
            &input.running,
        )
        .map_err(error_string)?;
    let workload = store
        .estimate_gpu_batch_workload_v1(&batch)
        .map_err(error_string)?;
    let burst = store
        .plan_gpu_burst_v1(&batch, &input.providers)
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

    let store = writable_store(&state)?;
    let burst = store
        .plan_gpu_burst_v1(&input.reviewed_batch, &input.providers)
        .map_err(error_string)?;
    if burst.schedule_hash != input.expected_schedule_hash {
        return Err(
            "The provider capability or current job state changed after review. Prepare the GPU batch again before starting Burst Mode."
                .to_owned(),
        );
    }
    if !burst.blocked.is_empty()
        || !burst.preflight_blocked_job_ids.is_empty()
        || burst.scheduled_job_count() != input.reviewed_batch.ready_jobs.len()
    {
        return Err(
            "Burst Mode was blocked by the canonical preflight re-check. Review the batch again."
                .to_owned(),
        );
    }
    if burst.policy.requires_human_prompt_after_start() {
        return Err("Burst Mode policy must remain non-interactive after start.".to_owned());
    }

    let queues = store
        .gpu_workbench_queue_snapshot_v1(&input.reviewed_batch.selected_project_ids)
        .map_err(error_string)?;

    Ok(GpuBurstStartViewV1 { burst, queues })
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
            llmgateway_status,
            save_llmgateway_settings,
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
