fn with_application_control_phase18<T>(
    state: &State<'_, DesktopState>,
    operation: impl FnOnce(
        &mut omnicreator_application::ApplicationControlService<'_>,
    ) -> omnicreator_application::ControlResultV1<T>,
) -> Result<T, String> {
    let guard = state.active.lock().map_err(lock_error)?;
    let active = guard
        .as_ref()
        .ok_or_else(|| "Open a Data Folder first.".to_owned())?;
    let mut service = match active {
        ActiveWorkspace::Writable(session) => {
            omnicreator_application::ApplicationControlService::for_writer(session)
        }
        ActiveWorkspace::ReadOnly(workspace) => {
            omnicreator_application::ApplicationControlService::for_read_only(workspace)
        }
    }
    .map_err(|error| error.to_string())?;
    operation(&mut service).map_err(|error| error.to_string())
}

#[tauri::command]
fn workflow_step_execution_policies(
    state: State<'_, DesktopState>,
    project_id: String,
) -> Result<Vec<omnicreator_core::WorkflowStepExecutionPolicyV1>, String> {
    with_application_control_phase18(&state, |service| {
        service
            .workflow_execution_policies_v1(&omnicreator_application::ProjectIdRequestV1 {
                project_id,
            })
            .map(|response| response.data)
    })
}

#[tauri::command]
fn set_workflow_step_automatic_execution(
    state: State<'_, DesktopState>,
    step_id: String,
    enabled: bool,
) -> Result<AppSnapshot, String> {
    with_application_control_phase18(&state, |service| {
        service
            .set_workflow_automatic_execution_v1(
                &omnicreator_application::SetWorkflowAutomaticExecutionRequestV1 {
                    step_id,
                    enabled,
                },
            )
            .map(|_| ())
    })?;
    snapshot_from_active(&state)
}

struct DesktopCreatorRunRuntimeV1<'a, 'b> {
    app: &'a AppHandle,
    state: &'a State<'b, DesktopState>,
    data_root: PathBuf,
}

impl DesktopCreatorRunRuntimeV1<'_, '_> {
    fn internal_error_v1(message: impl Into<String>) -> omnicreator_application::ControlErrorV1 {
        omnicreator_application::ControlErrorV1::new(
            omnicreator_application::ControlErrorCodeV1::Internal,
            message,
        )
    }

    fn provider_error_v1(message: impl Into<String>) -> omnicreator_application::ControlErrorV1 {
        omnicreator_application::ControlErrorV1::new(
            omnicreator_application::ControlErrorCodeV1::ProviderUnavailable,
            message,
        )
    }

    fn capability_error_v1(message: impl Into<String>) -> omnicreator_application::ControlErrorV1 {
        omnicreator_application::ControlErrorV1::new(
            omnicreator_application::ControlErrorCodeV1::CapabilityUnavailable,
            message,
        )
    }

    fn llm_v1(&self) -> omnicreator_application::ControlResultV1<LlmGatewayClient> {
        let config = load_llmgateway_config(self.app).map_err(Self::provider_error_v1)?;
        LlmGatewayClient::new(config).map_err(omnicreator_application::ControlErrorV1::from)
    }
}

impl omnicreator_application::CreatorRunRuntimeV1 for DesktopCreatorRunRuntimeV1<'_, '_> {
    fn resolve_studio_pack_v1(
        &mut self,
        studio_pack_id: &str,
    ) -> omnicreator_application::ControlResultV1<omnicreator_core::EffectiveStudioPackV1> {
        let catalog = load_studio_pack_catalog_v1(&self.data_root)
            .map_err(Self::capability_error_v1)?;
        catalog
            .resolve_v1(studio_pack_id)
            .map_err(omnicreator_application::ControlErrorV1::from)
    }

    fn run_content_v1(
        &mut self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        project_id: &str,
        input: &CreatorInputV1,
    ) -> omnicreator_application::ControlResultV1<()> {
        let llm = self.llm_v1()?;
        omnicreator_core::run_creator_content_stage_v1(
            state_store,
            artifact_store,
            &llm,
            project_id,
            input,
        )
        .map(|_| ())
        .map_err(omnicreator_application::ControlErrorV1::from)
    }

    fn run_scene_v1(
        &mut self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        project_id: &str,
        content: &omnicreator_core::CreatorContentV1,
        content_artifact: &Artifact,
        options: &CreatorContentSceneOptionsV1,
    ) -> omnicreator_application::ControlResultV1<()> {
        let llm = self.llm_v1()?;
        omnicreator_core::run_creator_scene_stage_v1(
            state_store,
            artifact_store,
            &llm,
            project_id,
            content,
            content_artifact,
            options,
        )
        .map(|_| ())
        .map_err(omnicreator_application::ControlErrorV1::from)
    }

    fn run_visual_v1(
        &mut self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        project: &Project,
        studio_pack: &omnicreator_core::EffectiveStudioPackV1,
        creator: &CreatorContentSceneOutcomeV1,
    ) -> omnicreator_application::ControlResultV1<bool> {
        let inventory = plugin_inventory_report_v1(self.app).map_err(Self::capability_error_v1)?;
        let plugin_runtime = studio_pack_runtime_snapshot_v1(self.app, &inventory.registry)
            .map_err(Self::capability_error_v1)?;
        let runtime_root =
            creator_plugin_runtime_root_v1(self.app).map_err(Self::internal_error_v1)?;
        let visual_runtime = DesktopVisualRuntimeV1 {
            registry: &inventory.registry,
            runtime: &plugin_runtime,
            runtime_root,
        };
        let visual_plan = plan_creator_visuals_v1(
            project,
            studio_pack,
            &creator.content,
            &creator.scene_plan,
            &creator.scene_plan_artifact.sha256,
            &visual_runtime,
            &CreatorVisualPlanningOptionsV1::default(),
        )
        .map_err(omnicreator_application::ControlErrorV1::from)?;
        execute_creator_visual_plan_v1(
            state_store,
            artifact_store,
            &visual_plan,
            &creator.scene_plan,
            &visual_runtime,
        )
        .map(|outcome| outcome.completed)
        .map_err(omnicreator_application::ControlErrorV1::from)
    }

    fn run_voice_v1(
        &mut self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        studio_pack: &omnicreator_core::EffectiveStudioPackV1,
        content: &omnicreator_core::CreatorContentV1,
    ) -> omnicreator_application::ControlResultV1<bool> {
        let provider_snapshot = {
            let mut guard = self
                .state
                .compute
                .lock()
                .map_err(|error| Self::internal_error_v1(error.to_string()))?;
            if let Some(runtime) = guard.as_mut() {
                let _ = runtime.heartbeat(Utc::now());
                let connection_state = runtime.state();
                if let Some(session) = runtime.session().cloned() {
                    let staging_dir =
                        compute_staging_dir(self.app).map_err(Self::internal_error_v1)?;
                    let _ = reconcile_remote_session_v1(
                        state_store,
                        artifact_store,
                        runtime.provider_mut(),
                        &session.identity.provider_id,
                        &session.identity.session_id,
                        connection_state,
                        staging_dir,
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
        let voice_runtime = creator_voice_runtime_v1(self.app, studio_pack, provider_snapshot.as_ref())
            .map_err(Self::capability_error_v1)?;
        let voice_plan = plan_creator_voice_orchestration_v1(
            state_store,
            artifact_store,
            content,
            &voice_runtime,
            &providers,
        )
        .map_err(omnicreator_application::ControlErrorV1::from)?;

        if voice_plan.all_complete() {
            return Ok(true);
        }
        if voice_plan.burst.scheduled_job_count() > 0 {
            let mut guard = self
                .state
                .compute
                .lock()
                .map_err(|error| Self::internal_error_v1(error.to_string()))?;
            if let Some(runtime) = guard.as_mut() {
                if runtime.state() == ComputeProviderConnectionState::Ready {
                    let _ = dispatch_creator_voice_burst_v1(
                        state_store,
                        runtime.provider_mut(),
                        &voice_plan,
                    )
                    .map_err(omnicreator_application::ControlErrorV1::from)?;
                }
            }
        }
        Ok(false)
    }
}

#[tauri::command]
fn start_creator_production_phase17(
    app: AppHandle,
    state: State<'_, DesktopState>,
    project_id: String,
    input_kind: String,
    input_text: String,
) -> Result<AppSnapshot, String> {
    let input = creator_input_v1(&input_kind, &input_text)?;
    let data_root = active_data_root(&state)?;
    let mut runtime = DesktopCreatorRunRuntimeV1 {
        app: &app,
        state: &state,
        data_root,
    };
    let request = omnicreator_application::StartOrResumeCreatorRequestV1 {
        project_id,
        input,
    };

    {
        let guard = state.active.lock().map_err(lock_error)?;
        let active = guard
            .as_ref()
            .ok_or_else(|| "Open a Data Folder first.".to_owned())?;
        let mut control = match active {
            ActiveWorkspace::Writable(session) => {
                omnicreator_application::CreatorRunControlServiceV1::for_writer(session)
            }
            ActiveWorkspace::ReadOnly(workspace) => {
                omnicreator_application::CreatorRunControlServiceV1::for_read_only(workspace)
            }
        }
        .map_err(|error| error.to_string())?;
        control
            .start_or_resume_v1(&request, &mut runtime)
            .map_err(|error| error.to_string())?;
    }

    snapshot_from_active(&state)
}

pub(super) fn run_phase17() {
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
            creator_visual_review_status,
            creator_manual_visual_status,
            creator_manual_voice_status,
            select_creator_visual_candidate,
            approve_creator_generated_visual,
            use_creator_image_manually,
            use_creator_video_manually,
            choose_creator_asset_library_visual,
            export_creator_external_visual_request,
            provide_creator_external_visual_result,
            use_creator_wav_manually,
            use_creator_audio_with_timing_manually,
            export_creator_external_voice_request,
            provide_creator_external_voice_result,
            replace_creator_voice_timing_manually,
            provide_creator_script_manually,
            import_creator_script_manually,
            creator_manual_scene_editor,
            provide_creator_scene_plan_manually,
            import_creator_scene_plan_manually,
            start_creator_production_phase17,
            workflow_step_execution_policies,
            set_workflow_step_automatic_execution,
            rename_project,
            delete_project,
            production_export_status,
            production_recovery_status,
            repair_production_visual,
            repair_production_audio,
            repair_production_timing,
            repair_production_voice_bundle,
            rebuild_recovered_production,
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
