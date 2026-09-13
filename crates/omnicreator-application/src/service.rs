use omnicreator_core::{
    approve_creator_generated_visual_v1, assemble_creator_production_pack_v1,
    build_studio_review_center_v1, choose_creator_visual_from_asset_library_v1,
    compile_creator_workflow_plan_v1, creator_scene_visual_states_v1,
    creator_segment_voice_states_v1, derive_creator_run_coordinator_v1,
    import_manual_creator_scene_plan_file_v1, import_manual_creator_script_file_v1,
    inspect_creator_production_recovery_v1, load_latest_creator_content_scene_v1,
    load_latest_creator_content_v1, load_latest_creator_production_pack_v1,
    materialize_creator_workflow_plan_v1, prepare_external_generated_visual_request_v1,
    prepare_external_voice_request_v1, project_board_projection_v1,
    provide_external_generated_visual_result_v1, provide_external_voice_result_v1,
    provide_manual_creator_content_v1, provide_manual_creator_scene_plan_v1,
    provide_manual_creator_visual_file_v1, provide_manual_creator_voice_bundle_v1,
    rebuild_and_export_creator_production_v1, repair_creator_production_audio_v1,
    repair_creator_production_timing_v1, repair_creator_production_visual_v1,
    repair_creator_production_voice_bundle_v1, replace_manual_creator_voice_timing_v1,
    select_creator_stock_candidate_v1, ArtifactStore, CreatorContentV1,
    CreatorProductionPackOptionsV1, CreatorProductionPackOutcomeV1, CreatorSceneVisualStateV1,
    CreatorSegmentVoiceStateV1, CreatorVisualPlanV1, EffectiveStudioPackV1,
    ExternalGeneratedVisualRequestV1, ExternalVoiceRequestV1, ManualCreatorContentOutcomeV1,
    ManualCreatorScenePlanOutcomeV1, ManualCreatorVisualOutcomeV1, ManualCreatorVoiceOutcomeV1,
    ManualCreatorVoiceRequestV1, ManualScenePlanDraftV1, ProductionRecoveryRebuildOutcomeV1,
    ProductionRecoveryViewV1, Project, StateStore, StudioJobReviewSnapshotV1,
    StudioReviewCenterV1, StudioReviewProjectSnapshotV1, WorkflowStepExecutionPolicyV1, Workspace,
    WorkspaceSession, CREATOR_STEP_CONTENT_PREPARE_V1, CREATOR_STEP_PRODUCTION_PACK_V1,
    CREATOR_STEP_SCENE_PLAN_V1, CREATOR_STEP_VISUAL_PREPARE_V1, CREATOR_STEP_VOICE_PREPARE_V1,
    CREATOR_WORKFLOW_UNIT_PROJECT_V1,
};

use crate::{
    ApplicationRuntimeInspectorV1, AssetLibraryVisualRequestV1, BindStudioPackRequestV1,
    ComputeRuntimeControlSnapshotV1, ControlAccessV1, ControlErrorCodeV1, ControlErrorV1,
    ControlOperationV1, ControlResponseV1, ControlResultV1, CreateProjectRequestV1,
    ExternalVisualResultRequestV1, ExternalVoiceResultRequestV1, ManualContentImportRequestV1,
    ManualContentRequestV1, ManualScenePlanImportRequestV1, ManualScenePlanRequestV1,
    ManualVisualRequestV1, ManualVoiceRequestV1, PluginRuntimeControlSnapshotV1,
    ProductionRecoveryControlSnapshotV1, ProjectControlSnapshotV1, ProjectIdRequestV1,
    ProjectListControlSnapshotV1, RecoveryFileRequestV1, RecoveryVoiceBundleRequestV1,
    RenameProjectRequestV1, ReplaceVoiceTimingRequestV1, RuntimeInspectionSnapshotV1,
    SetWorkflowAutomaticExecutionRequestV1, WorkspaceControlSnapshotV1, CONTROL_CONTRACT_SCHEMA_V1,
    CONTROL_CONTRACT_VERSION_V1,
};

pub struct ApplicationControlService<'a> {
    workspace: &'a Workspace,
    store: StateStore,
    artifacts: ArtifactStore,
    access: ControlAccessV1,
}

impl<'a> ApplicationControlService<'a> {
    pub fn for_writer(session: &'a WorkspaceSession) -> ControlResultV1<Self> {
        let workspace = session.workspace();
        Ok(Self {
            workspace,
            store: StateStore::open(session.sqlite_path()).map_err(ControlErrorV1::from)?,
            artifacts: ArtifactStore::new(workspace.data_root()).map_err(ControlErrorV1::from)?,
            access: ControlAccessV1::Writable,
        })
    }

    pub fn for_read_only(workspace: &'a Workspace) -> ControlResultV1<Self> {
        Ok(Self {
            workspace,
            store: StateStore::open_read_only(workspace.sqlite_path())
                .map_err(ControlErrorV1::from)?,
            artifacts: ArtifactStore::new(workspace.data_root()).map_err(ControlErrorV1::from)?,
            access: ControlAccessV1::ReadOnly,
        })
    }

    pub fn access_v1(&self) -> ControlAccessV1 {
        self.access
    }

    pub fn workspace_status_v1(
        &self,
    ) -> ControlResultV1<ControlResponseV1<WorkspaceControlSnapshotV1>> {
        let manifest = self.workspace.manifest();
        let project_count = self
            .store
            .list_projects()
            .map_err(ControlErrorV1::from)?
            .len();
        Ok(ControlResponseV1::new(
            ControlOperationV1::WorkspaceStatus,
            WorkspaceControlSnapshotV1 {
                schema: CONTROL_CONTRACT_SCHEMA_V1.to_owned(),
                version: CONTROL_CONTRACT_VERSION_V1,
                workspace_id: manifest.workspace_id.clone(),
                revision: manifest.revision,
                access: self.access,
                read_only: self.access.is_read_only(),
                project_count,
            },
        ))
    }

    pub fn list_projects_v1(
        &self,
    ) -> ControlResultV1<ControlResponseV1<ProjectListControlSnapshotV1>> {
        let projects = self
            .store
            .list_projects()
            .map_err(ControlErrorV1::from)?
            .into_iter()
            .map(|project| self.project_snapshot_v1(project))
            .collect::<ControlResultV1<Vec<_>>>()?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::ListProjects,
            ProjectListControlSnapshotV1 {
                schema: CONTROL_CONTRACT_SCHEMA_V1.to_owned(),
                version: CONTROL_CONTRACT_VERSION_V1,
                read_only: self.access.is_read_only(),
                projects,
            },
        ))
    }

    pub fn project_status_v1(
        &self,
        request: &ProjectIdRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ProjectControlSnapshotV1>> {
        let project = self
            .store
            .get_project(&request.project_id)
            .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::ProjectStatus,
            self.project_snapshot_v1(project)?,
        ))
    }

    pub fn review_center_v1(&self) -> ControlResultV1<ControlResponseV1<StudioReviewCenterV1>> {
        let projects = self.store.list_projects().map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::ReviewCenter,
            self.review_center_for_projects_v1(&projects)?,
        ))
    }

    pub fn creator_run_state_v1(
        &self,
        request: &ProjectIdRequestV1,
    ) -> ControlResultV1<ControlResponseV1<omnicreator_core::CreatorRunCoordinatorV1>> {
        let run =
            derive_creator_run_coordinator_v1(&self.store, &self.artifacts, &request.project_id)
                .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::CreatorRunState,
            run,
        ))
    }

    pub fn create_project_v1(
        &mut self,
        request: &CreateProjectRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ProjectControlSnapshotV1>> {
        self.require_writable_v1()?;
        let title = non_empty_v1("project title", &request.title)?;
        let project = self
            .store
            .create_project(title)
            .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::CreateProject,
            self.project_snapshot_v1(project)?,
        ))
    }

    pub fn create_creator_project_v1(
        &mut self,
        title: &str,
        studio_pack: &EffectiveStudioPackV1,
    ) -> ControlResultV1<ControlResponseV1<ProjectControlSnapshotV1>> {
        self.require_writable_v1()?;
        let title = non_empty_v1("project title", title)?;
        studio_pack.validate_v1().map_err(ControlErrorV1::from)?;
        let project = self
            .store
            .create_project_with_studio_pack(title, Some(&studio_pack.id))
            .map_err(ControlErrorV1::from)?;
        let plan = compile_creator_workflow_plan_v1(&project, studio_pack)
            .map_err(ControlErrorV1::from)?;
        materialize_creator_workflow_plan_v1(&self.store, &plan).map_err(ControlErrorV1::from)?;
        let project = self
            .store
            .get_project(&project.id)
            .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::CreateCreatorProject,
            self.project_snapshot_v1(project)?,
        ))
    }

    pub fn rename_project_v1(
        &mut self,
        request: &RenameProjectRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ProjectControlSnapshotV1>> {
        self.require_writable_v1()?;
        let title = non_empty_v1("project title", &request.title)?;
        let project = self
            .store
            .update_project_title(&request.project_id, title)
            .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::RenameProject,
            self.project_snapshot_v1(project)?,
        ))
    }

    pub fn delete_project_v1(
        &mut self,
        request: &ProjectIdRequestV1,
    ) -> ControlResultV1<ControlResponseV1<String>> {
        self.require_writable_v1()?;
        self.store
            .delete_project(&request.project_id)
            .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::DeleteProject,
            request.project_id.clone(),
        ))
    }

    pub fn bind_studio_pack_v1(
        &mut self,
        request: &BindStudioPackRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ProjectControlSnapshotV1>> {
        self.require_writable_v1()?;
        if request
            .studio_pack_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ControlErrorV1::new(
                ControlErrorCodeV1::InvalidInput,
                "studio_pack_id must not be empty when present",
            ));
        }
        let project = self
            .store
            .update_project_studio_pack(&request.project_id, request.studio_pack_id.as_deref())
            .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::BindStudioPack,
            self.project_snapshot_v1(project)?,
        ))
    }

    pub fn workflow_execution_policies_v1(
        &self,
        request: &ProjectIdRequestV1,
    ) -> ControlResultV1<ControlResponseV1<Vec<WorkflowStepExecutionPolicyV1>>> {
        let policies = self
            .store
            .list_project_workflow_step_execution_policies_v1(&request.project_id)
            .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::WorkflowExecutionPolicies,
            policies,
        ))
    }

    pub fn set_workflow_automatic_execution_v1(
        &mut self,
        request: &SetWorkflowAutomaticExecutionRequestV1,
    ) -> ControlResultV1<ControlResponseV1<WorkflowStepExecutionPolicyV1>> {
        self.require_writable_v1()?;
        let policy = self
            .store
            .set_workflow_step_automatic_execution_v1(&request.step_id, request.enabled)
            .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::SetWorkflowAutomaticExecution,
            policy,
        ))
    }

    pub fn provide_manual_content_v1(
        &mut self,
        request: &ManualContentRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ManualCreatorContentOutcomeV1>> {
        self.require_writable_v1()?;
        let outcome = provide_manual_creator_content_v1(
            &mut self.store,
            &self.artifacts,
            &request.project_id,
            &request.script,
            request.provenance.clone(),
        )
        .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::ProvideManualContent,
            outcome,
        ))
    }

    pub fn import_manual_content_v1(
        &mut self,
        request: &ManualContentImportRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ManualCreatorContentOutcomeV1>> {
        self.require_writable_v1()?;
        let outcome = import_manual_creator_script_file_v1(
            &mut self.store,
            &self.artifacts,
            &request.project_id,
            &request.source_path,
        )
        .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::ImportManualContent,
            outcome,
        ))
    }

    pub fn scene_plan_editor_v1(
        &self,
        request: &ProjectIdRequestV1,
    ) -> ControlResultV1<(CreatorContentV1, String, ManualScenePlanDraftV1)> {
        let (content, content_artifact) =
            load_latest_creator_content_v1(&self.store, &self.artifacts, &request.project_id)
                .map_err(ControlErrorV1::from)?
                .ok_or_else(|| {
                    ControlErrorV1::new(
                        ControlErrorCodeV1::Blocked,
                        "verified creator Content is required before ScenePlan editing",
                    )
                })?;
        let existing =
            load_latest_creator_content_scene_v1(&self.store, &self.artifacts, &request.project_id)
                .map_err(ControlErrorV1::from)?;
        let draft = existing
            .filter(|value| value.content_artifact.artifact_id == content_artifact.artifact_id)
            .map(|value| ManualScenePlanDraftV1::from_canonical_v1(&value.scene_plan))
            .unwrap_or(
                ManualScenePlanDraftV1::for_content_v1(&content).map_err(ControlErrorV1::from)?,
            );
        Ok((content, content_artifact.sha256, draft))
    }

    pub fn provide_manual_scene_plan_v1(
        &mut self,
        request: &ManualScenePlanRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ManualCreatorScenePlanOutcomeV1>> {
        self.require_writable_v1()?;
        let outcome = provide_manual_creator_scene_plan_v1(
            &mut self.store,
            &self.artifacts,
            &request.project_id,
            &request.draft,
            request.provenance.clone(),
        )
        .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::ProvideManualScenePlan,
            outcome,
        ))
    }

    pub fn import_manual_scene_plan_v1(
        &mut self,
        request: &ManualScenePlanImportRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ManualCreatorScenePlanOutcomeV1>> {
        self.require_writable_v1()?;
        let outcome = import_manual_creator_scene_plan_file_v1(
            &mut self.store,
            &self.artifacts,
            &request.project_id,
            &request.source_path,
        )
        .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::ImportManualScenePlan,
            outcome,
        ))
    }

    pub fn visual_states_v1(
        &self,
        request: &ProjectIdRequestV1,
    ) -> ControlResultV1<Vec<CreatorSceneVisualStateV1>> {
        creator_scene_visual_states_v1(&self.store, &self.artifacts, &request.project_id)
            .map_err(ControlErrorV1::from)
    }

    pub fn provide_manual_visual_v1(
        &mut self,
        request: &ManualVisualRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ManualCreatorVisualOutcomeV1>> {
        self.require_writable_v1()?;
        let outcome = provide_manual_creator_visual_file_v1(
            &mut self.store,
            &self.artifacts,
            &request.project_id,
            &request.scene_id,
            &request.source_path,
            request.provenance.clone(),
            request.replace_existing,
        )
        .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::ProvideManualVisual,
            outcome,
        ))
    }

    pub fn choose_asset_library_visual_v1(
        &mut self,
        request: &AssetLibraryVisualRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ManualCreatorVisualOutcomeV1>> {
        self.require_writable_v1()?;
        let outcome = choose_creator_visual_from_asset_library_v1(
            &mut self.store,
            &self.artifacts,
            &request.project_id,
            &request.scene_id,
            &request.artifact_id,
            request.replace_existing,
        )
        .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::ChooseAssetLibraryVisual,
            outcome,
        ))
    }

    pub fn select_stock_candidate_v1(
        &self,
        mut plan: CreatorVisualPlanV1,
        scene_id: &str,
        candidate_id: &str,
    ) -> ControlResultV1<ControlResponseV1<CreatorVisualPlanV1>> {
        select_creator_stock_candidate_v1(&mut plan, scene_id, candidate_id)
            .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::SelectStockCandidate,
            plan,
        ))
    }

    pub fn approve_generated_visual_v1(
        &self,
        mut plan: CreatorVisualPlanV1,
        scene_id: &str,
    ) -> ControlResultV1<ControlResponseV1<CreatorVisualPlanV1>> {
        approve_creator_generated_visual_v1(&mut plan, scene_id).map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::ApproveGeneratedVisual,
            plan,
        ))
    }

    pub fn prepare_external_visual_v1(
        &self,
        project_id: &str,
        scene_id: &str,
    ) -> ControlResultV1<ControlResponseV1<ExternalGeneratedVisualRequestV1>> {
        let request = prepare_external_generated_visual_request_v1(
            &self.store,
            &self.artifacts,
            project_id,
            scene_id,
        )
        .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::PrepareExternalVisual,
            request,
        ))
    }

    pub fn provide_external_visual_v1(
        &mut self,
        request: &ExternalVisualResultRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ManualCreatorVisualOutcomeV1>> {
        self.require_writable_v1()?;
        let outcome = provide_external_generated_visual_result_v1(
            &mut self.store,
            &self.artifacts,
            &request.request,
            &request.source_path,
            request.source_label.clone(),
            request.replace_existing,
        )
        .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::ProvideExternalVisual,
            outcome,
        ))
    }

    pub fn voice_states_v1(
        &self,
        request: &ProjectIdRequestV1,
    ) -> ControlResultV1<Vec<CreatorSegmentVoiceStateV1>> {
        creator_segment_voice_states_v1(&self.store, &self.artifacts, &request.project_id)
            .map_err(ControlErrorV1::from)
    }

    pub fn provide_manual_voice_v1(
        &mut self,
        request: &ManualVoiceRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ManualCreatorVoiceOutcomeV1>> {
        self.require_writable_v1()?;
        let outcome = provide_manual_creator_voice_bundle_v1(
            &mut self.store,
            &self.artifacts,
            ManualCreatorVoiceRequestV1 {
                project_id: &request.project_id,
                segment_id: &request.segment_id,
                audio_path: &request.audio_path,
                timing: request.timing.clone(),
                provenance: request.provenance.clone(),
                replace_existing: request.replace_existing,
            },
        )
        .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::ProvideManualVoice,
            outcome,
        ))
    }

    pub fn replace_voice_timing_v1(
        &mut self,
        request: &ReplaceVoiceTimingRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ManualCreatorVoiceOutcomeV1>> {
        self.require_writable_v1()?;
        let outcome = replace_manual_creator_voice_timing_v1(
            &mut self.store,
            &self.artifacts,
            &request.project_id,
            &request.segment_id,
            request.timing.clone(),
            request.provenance.clone(),
        )
        .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::ReplaceVoiceTiming,
            outcome,
        ))
    }

    pub fn prepare_external_voice_v1(
        &self,
        project_id: &str,
        segment_id: &str,
    ) -> ControlResultV1<ControlResponseV1<ExternalVoiceRequestV1>> {
        let request =
            prepare_external_voice_request_v1(&self.store, &self.artifacts, project_id, segment_id)
                .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::PrepareExternalVoice,
            request,
        ))
    }

    pub fn provide_external_voice_v1(
        &mut self,
        request: &ExternalVoiceResultRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ManualCreatorVoiceOutcomeV1>> {
        self.require_writable_v1()?;
        let outcome = provide_external_voice_result_v1(
            &mut self.store,
            &self.artifacts,
            &request.request,
            &request.audio_path,
            request.timing.clone(),
            request.source_label.clone(),
            request.replace_existing,
        )
        .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::ProvideExternalVoice,
            outcome,
        ))
    }

    pub fn production_recovery_v1(
        &self,
        request: &ProjectIdRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ProductionRecoveryControlSnapshotV1>> {
        let recovery = inspect_creator_production_recovery_v1(
            &self.store,
            &self.artifacts,
            &request.project_id,
        )
        .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::InspectProductionRecovery,
            ProductionRecoveryControlSnapshotV1 {
                read_only: self.access.is_read_only(),
                recovery,
            },
        ))
    }

    pub fn repair_production_visual_v1(
        &mut self,
        request: &RecoveryFileRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ProductionRecoveryViewV1>> {
        self.require_writable_v1()?;
        repair_creator_production_visual_v1(
            &mut self.store,
            &self.artifacts,
            &request.project_id,
            &request.canonical_id,
            &request.source_path,
        )
        .map_err(ControlErrorV1::from)?;
        self.recovery_after_mutation_v1(
            &request.project_id,
            ControlOperationV1::RepairProductionVisual,
        )
    }

    pub fn repair_production_audio_v1(
        &mut self,
        request: &RecoveryFileRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ProductionRecoveryViewV1>> {
        self.require_writable_v1()?;
        repair_creator_production_audio_v1(
            &mut self.store,
            &self.artifacts,
            &request.project_id,
            &request.canonical_id,
            &request.source_path,
        )
        .map_err(ControlErrorV1::from)?;
        self.recovery_after_mutation_v1(
            &request.project_id,
            ControlOperationV1::RepairProductionAudio,
        )
    }

    pub fn repair_production_timing_v1(
        &mut self,
        request: &RecoveryFileRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ProductionRecoveryViewV1>> {
        self.require_writable_v1()?;
        repair_creator_production_timing_v1(
            &mut self.store,
            &self.artifacts,
            &request.project_id,
            &request.canonical_id,
            &request.source_path,
        )
        .map_err(ControlErrorV1::from)?;
        self.recovery_after_mutation_v1(
            &request.project_id,
            ControlOperationV1::RepairProductionTiming,
        )
    }

    pub fn repair_production_voice_bundle_v1(
        &mut self,
        request: &RecoveryVoiceBundleRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ProductionRecoveryViewV1>> {
        self.require_writable_v1()?;
        repair_creator_production_voice_bundle_v1(
            &mut self.store,
            &self.artifacts,
            &request.project_id,
            &request.segment_id,
            &request.audio_path,
            &request.timing_path,
        )
        .map_err(ControlErrorV1::from)?;
        self.recovery_after_mutation_v1(
            &request.project_id,
            ControlOperationV1::RepairProductionVoiceBundle,
        )
    }

    pub fn assemble_production_pack_v1(
        &mut self,
        request: &ProjectIdRequestV1,
    ) -> ControlResultV1<ControlResponseV1<CreatorProductionPackOutcomeV1>> {
        self.require_writable_v1()?;
        let outcome = assemble_creator_production_pack_v1(
            &mut self.store,
            &self.artifacts,
            &request.project_id,
            &CreatorProductionPackOptionsV1::default(),
        )
        .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::AssembleProductionPack,
            outcome,
        ))
    }

    pub fn latest_production_pack_v1(
        &self,
        request: &ProjectIdRequestV1,
    ) -> ControlResultV1<Option<CreatorProductionPackOutcomeV1>> {
        load_latest_creator_production_pack_v1(&self.store, &self.artifacts, &request.project_id)
            .map_err(ControlErrorV1::from)
    }

    pub fn rebuild_and_export_production_v1(
        &mut self,
        request: &ProjectIdRequestV1,
    ) -> ControlResultV1<ControlResponseV1<ProductionRecoveryRebuildOutcomeV1>> {
        self.require_writable_v1()?;
        let outcome = rebuild_and_export_creator_production_v1(
            &mut self.store,
            &self.artifacts,
            &request.project_id,
        )
        .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(
            ControlOperationV1::RebuildAndExportProduction,
            outcome,
        ))
    }

    pub fn runtime_inspection_v1(
        &self,
        inspector: &impl ApplicationRuntimeInspectorV1,
    ) -> ControlResultV1<RuntimeInspectionSnapshotV1> {
        Ok(RuntimeInspectionSnapshotV1 {
            plugin: inspector.plugin_runtime_snapshot_v1()?,
            compute: inspector.compute_runtime_snapshot_v1()?,
        })
    }

    pub fn plugin_runtime_inspection_v1(
        &self,
        inspector: &impl ApplicationRuntimeInspectorV1,
    ) -> ControlResultV1<ControlResponseV1<PluginRuntimeControlSnapshotV1>> {
        Ok(ControlResponseV1::new(
            ControlOperationV1::PluginRuntimeInspection,
            inspector.plugin_runtime_snapshot_v1()?,
        ))
    }

    pub fn compute_runtime_inspection_v1(
        &self,
        inspector: &impl ApplicationRuntimeInspectorV1,
    ) -> ControlResultV1<ControlResponseV1<ComputeRuntimeControlSnapshotV1>> {
        Ok(ControlResponseV1::new(
            ControlOperationV1::ComputeRuntimeInspection,
            inspector.compute_runtime_snapshot_v1()?,
        ))
    }

    fn require_writable_v1(&self) -> ControlResultV1<()> {
        if self.access.is_read_only() {
            Err(ControlErrorV1::read_only())
        } else {
            Ok(())
        }
    }

    fn project_snapshot_v1(&self, project: Project) -> ControlResultV1<ProjectControlSnapshotV1> {
        let steps = self
            .store
            .list_project_steps(&project.id)
            .map_err(ControlErrorV1::from)?;
        let jobs = self
            .store
            .list_project_jobs(&project.id)
            .map_err(ControlErrorV1::from)?;
        let status = self
            .store
            .derive_project_status(&project.id)
            .map_err(ControlErrorV1::from)?;
        let board = project_board_projection_v1(status, &jobs, &steps);
        let execution_policies = self
            .store
            .list_project_workflow_step_execution_policies_v1(&project.id)
            .map_err(ControlErrorV1::from)?;
        let has_creator_dag = [
            CREATOR_STEP_CONTENT_PREPARE_V1,
            CREATOR_STEP_SCENE_PLAN_V1,
            CREATOR_STEP_VISUAL_PREPARE_V1,
            CREATOR_STEP_VOICE_PREPARE_V1,
            CREATOR_STEP_PRODUCTION_PACK_V1,
        ]
        .iter()
        .all(|key| {
            steps
                .iter()
                .any(|step| step.step == *key && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1)
        });
        let run_coordinator = if has_creator_dag {
            Some(
                derive_creator_run_coordinator_v1(&self.store, &self.artifacts, &project.id)
                    .map_err(ControlErrorV1::from)?,
            )
        } else {
            None
        };
        let review_center = self.review_center_for_projects_v1(std::slice::from_ref(&project))?;
        Ok(ProjectControlSnapshotV1 {
            schema: CONTROL_CONTRACT_SCHEMA_V1.to_owned(),
            version: CONTROL_CONTRACT_VERSION_V1,
            project,
            status,
            board,
            steps,
            jobs,
            execution_policies,
            run_coordinator,
            review_center,
        })
    }

    fn review_center_for_projects_v1(
        &self,
        projects: &[Project],
    ) -> ControlResultV1<StudioReviewCenterV1> {
        let mut snapshots = Vec::<StudioReviewProjectSnapshotV1>::new();
        for project in projects {
            let jobs = self
                .store
                .list_project_jobs(&project.id)
                .map_err(ControlErrorV1::from)?
                .into_iter()
                .map(|job| {
                    let attempts = self
                        .store
                        .list_attempts(&job.job_id)
                        .map_err(ControlErrorV1::from)?;
                    Ok(StudioJobReviewSnapshotV1 { job, attempts })
                })
                .collect::<ControlResultV1<Vec<_>>>()?;
            let steps = self
                .store
                .list_project_steps(&project.id)
                .map_err(ControlErrorV1::from)?;
            snapshots.push((project.clone(), jobs, steps, None));
        }
        Ok(build_studio_review_center_v1(&snapshots))
    }

    fn recovery_after_mutation_v1(
        &self,
        project_id: &str,
        operation: ControlOperationV1,
    ) -> ControlResultV1<ControlResponseV1<ProductionRecoveryViewV1>> {
        let recovery =
            inspect_creator_production_recovery_v1(&self.store, &self.artifacts, project_id)
                .map_err(ControlErrorV1::from)?;
        Ok(ControlResponseV1::new(operation, recovery))
    }
}

fn non_empty_v1<'a>(label: &str, value: &'a str) -> ControlResultV1<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ControlErrorV1::new(
            ControlErrorCodeV1::InvalidInput,
            format!("{label} must not be empty"),
        ));
    }
    Ok(value)
}
