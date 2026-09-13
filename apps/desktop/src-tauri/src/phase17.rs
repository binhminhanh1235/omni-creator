#[tauri::command]
pub(super) fn workflow_step_execution_policies(
    state: State<'_, DesktopState>,
    project_id: String,
) -> Result<Vec<omnicreator_core::WorkflowStepExecutionPolicyV1>, String> {
    readable_store(&state)?
        .list_project_workflow_step_execution_policies_v1(&project_id)
        .map_err(error_string)
}

#[tauri::command]
pub(super) fn set_workflow_step_automatic_execution(
    state: State<'_, DesktopState>,
    step_id: String,
    enabled: bool,
) -> Result<AppSnapshot, String> {
    let store = writable_store(&state)?;
    store
        .set_workflow_step_automatic_execution_v1(&step_id, enabled)
        .map_err(error_string)?;
    drop(store);
    snapshot_from_active(&state)
}

fn workflow_step_auto_enabled_phase17(
    store: &StateStore,
    project_id: &str,
    step_key: &str,
) -> Result<bool, String> {
    store
        .workflow_step_automatic_execution_enabled_v1(
            project_id,
            step_key,
            omnicreator_core::CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        )
        .map_err(error_string)
}

#[tauri::command]
pub(super) fn start_creator_production_phase17(
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

    let mut content_state = load_latest_creator_content_v1(&store, &artifacts, &project_id)
        .map_err(error_string)?;
    let content_input_changed = match (&requested_input, &content_state) {
        (Some(input), Some((content, _))) => content.source != *input,
        (Some(_), None) => true,
        (None, _) => false,
    };

    if content_state.is_none() || content_input_changed {
        if !workflow_step_auto_enabled_phase17(
            &store,
            &project_id,
            omnicreator_core::CREATOR_STEP_CONTENT_PREPARE_V1,
        )? {
            drop(store);
            return snapshot_from_active(&state);
        }
        let input = requested_input.as_ref().ok_or_else(|| {
            "Creator topic or script is required until Content has a verified canonical artifact."
                .to_owned()
        })?;
        let llm = LlmGatewayClient::new(load_llmgateway_config(&app)?).map_err(error_string)?;
        let (content, artifact, _) = omnicreator_core::run_creator_content_stage_v1(
            &mut store,
            &artifacts,
            &llm,
            &project_id,
            input,
        )
        .map_err(error_string)?;
        content_state = Some((content, artifact));
    }

    let (content, content_artifact) = content_state.ok_or_else(|| {
        "Creator topic or script is required until Content has a verified canonical artifact."
            .to_owned()
    })?;

    let mut creator = load_latest_creator_content_scene_v1(&store, &artifacts, &project_id)
        .map_err(error_string)?
        .filter(|value| value.content_artifact.artifact_id == content_artifact.artifact_id);
    if creator.is_none() {
        if !workflow_step_auto_enabled_phase17(
            &store,
            &project_id,
            omnicreator_core::CREATOR_STEP_SCENE_PLAN_V1,
        )? {
            drop(store);
            return snapshot_from_active(&state);
        }
        let llm = LlmGatewayClient::new(load_llmgateway_config(&app)?).map_err(error_string)?;
        omnicreator_core::run_creator_scene_stage_v1(
            &mut store,
            &artifacts,
            &llm,
            &project_id,
            &content,
            &content_artifact,
            &CreatorContentSceneOptionsV1::default(),
        )
        .map_err(error_string)?;
        creator = load_latest_creator_content_scene_v1(&store, &artifacts, &project_id)
            .map_err(error_string)?;
    }
    let creator = creator.ok_or_else(|| {
        "Creator Scene Plan state is unavailable after SceneIntent orchestration.".to_owned()
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
        if !workflow_step_auto_enabled_phase17(
            &store,
            &project_id,
            CREATOR_STEP_VISUAL_PREPARE_V1,
        )? {
            drop(store);
            return snapshot_from_active(&state);
        }
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

    let voice_step = store
        .list_project_steps(&project_id)
        .map_err(error_string)?
        .into_iter()
        .find(|step| {
            step.step == omnicreator_core::CREATOR_STEP_VOICE_PREPARE_V1
                && step.unit == omnicreator_core::CREATOR_WORKFLOW_UNIT_PROJECT_V1
        })
        .ok_or_else(|| "Creator voice workflow step is missing.".to_owned())?;
    if voice_step.status != omnicreator_core::StepStatus::Succeeded
        && !workflow_step_auto_enabled_phase17(
            &store,
            &project_id,
            omnicreator_core::CREATOR_STEP_VOICE_PREPARE_V1,
        )?
    {
        drop(store);
        return snapshot_from_active(&state);
    }

    if voice_step.status != omnicreator_core::StepStatus::Succeeded {
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
    }

    let production_step = store
        .list_project_steps(&project_id)
        .map_err(error_string)?
        .into_iter()
        .find(|step| {
            step.step == omnicreator_core::CREATOR_STEP_PRODUCTION_PACK_V1
                && step.unit == omnicreator_core::CREATOR_WORKFLOW_UNIT_PROJECT_V1
        })
        .ok_or_else(|| "Creator Production Pack workflow step is missing.".to_owned())?;
    if production_step.status != omnicreator_core::StepStatus::Succeeded
        && !workflow_step_auto_enabled_phase17(
            &store,
            &project_id,
            omnicreator_core::CREATOR_STEP_PRODUCTION_PACK_V1,
        )?
    {
        drop(store);
        return snapshot_from_active(&state);
    }

    if production_step.status != omnicreator_core::StepStatus::Succeeded {
        assemble_creator_production_pack_v1(
            &mut store,
            &artifacts,
            &project_id,
            &CreatorProductionPackOptionsV1::default(),
        )
        .map_err(error_string)?;
    }

    drop(store);
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
