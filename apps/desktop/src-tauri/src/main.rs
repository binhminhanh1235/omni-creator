use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use omnicreator_core::{
    Error as CoreError, HandoffManifest, MachineBinding, Project, ProjectDisplayStatus, StateStore,
    Workspace, WorkspaceSession,
};
use serde::Serialize;
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
fn create_project(
    state: State<'_, DesktopState>,
    title: String,
) -> Result<AppSnapshot, String> {
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
fn prepare_device_handoff(
    state: State<'_, DesktopState>,
) -> Result<AppSnapshot, String> {
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
    binding
        .save(binding_path(app)?)
        .map_err(error_string)?;

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
    let handoff_path = workspace.data_root().join(".omnicreator/handoff/latest.json");

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
