use std::fs;

use omnicreator_application::{
    ApplicationControlService, ControlErrorCodeV1, ControlErrorV1, ControlResultV1,
    CreatorRunControlServiceV1, CreatorRunRuntimeV1, ManualContentRequestV1,
    ManualScenePlanRequestV1, ManualVisualRequestV1, ManualVoiceRequestV1, ProjectIdRequestV1,
    SetWorkflowAutomaticExecutionRequestV1, StartOrResumeCreatorRequestV1,
};
use omnicreator_core::{
    derive_manual_voice_timing_v1, initial_studio_pack_catalog_v1, Artifact, ArtifactStore,
    CreatorContentSceneOptionsV1, CreatorContentSceneOutcomeV1, CreatorContentV1, CreatorInputV1,
    CreatorRunActionV1, EffectiveStudioPackV1, ManualResultProvenanceV1, Project, StateStore,
    StepStatus, Workspace, WorkspaceSession, CREATOR_STEP_CONTENT_PREPARE_V1,
    CREATOR_STEP_PRODUCTION_PACK_V1, CREATOR_WORKFLOW_UNIT_PROJECT_V1,
};

fn png() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&1280u32.to_be_bytes());
    bytes.extend_from_slice(&720u32.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
    bytes.extend_from_slice(&[0, 0, 0, 0, 1]);
    bytes
}

fn wav(duration_ms: u64) -> Vec<u8> {
    let sample_rate = 8_000u32;
    let channels = 1u16;
    let bits = 16u16;
    let block_align = channels * (bits / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let data_size = ((u64::from(byte_rate) * duration_ms) / 1_000) as u32;
    let mut out = Vec::with_capacity((44 + data_size) as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32 + data_size).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend(std::iter::repeat(7u8).take(data_size as usize));
    out
}

struct NoProviderRuntime {
    pack: EffectiveStudioPackV1,
}

impl NoProviderRuntime {
    fn unexpected(stage: &str) -> ControlErrorV1 {
        ControlErrorV1::new(
            ControlErrorCodeV1::Internal,
            format!("provider runtime must not be called for {stage} in this test"),
        )
    }
}

impl CreatorRunRuntimeV1 for NoProviderRuntime {
    fn resolve_studio_pack_v1(
        &mut self,
        studio_pack_id: &str,
    ) -> ControlResultV1<EffectiveStudioPackV1> {
        assert_eq!(studio_pack_id, self.pack.id);
        Ok(self.pack.clone())
    }

    fn run_content_v1(
        &mut self,
        _state_store: &mut StateStore,
        _artifact_store: &ArtifactStore,
        _project_id: &str,
        _input: &CreatorInputV1,
    ) -> ControlResultV1<()> {
        Err(Self::unexpected("content"))
    }

    fn run_scene_v1(
        &mut self,
        _state_store: &mut StateStore,
        _artifact_store: &ArtifactStore,
        _project_id: &str,
        _content: &CreatorContentV1,
        _content_artifact: &Artifact,
        _options: &CreatorContentSceneOptionsV1,
    ) -> ControlResultV1<()> {
        Err(Self::unexpected("scene"))
    }

    fn run_visual_v1(
        &mut self,
        _state_store: &mut StateStore,
        _artifact_store: &ArtifactStore,
        _project: &Project,
        _studio_pack: &EffectiveStudioPackV1,
        _creator: &CreatorContentSceneOutcomeV1,
    ) -> ControlResultV1<bool> {
        Err(Self::unexpected("visual"))
    }

    fn run_voice_v1(
        &mut self,
        _state_store: &mut StateStore,
        _artifact_store: &ArtifactStore,
        _studio_pack: &EffectiveStudioPackV1,
        _content: &CreatorContentV1,
    ) -> ControlResultV1<bool> {
        Err(Self::unexpected("voice"))
    }
}

#[test]
fn shared_start_resume_obeys_auto_off_before_provider_execution() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data-root")).unwrap();
    let session = WorkspaceSession::acquire(workspace, "phase18-run-policy").unwrap();
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let project = {
        let mut service = ApplicationControlService::for_writer(&session).unwrap();
        let project = service
            .create_creator_project_v1("Shared Start Resume Policy", &pack)
            .unwrap()
            .data
            .project;
        let snapshot = service
            .project_status_v1(&ProjectIdRequestV1 {
                project_id: project.id.clone(),
            })
            .unwrap()
            .data;
        let content_step = snapshot
            .steps
            .iter()
            .find(|step| {
                step.step == CREATOR_STEP_CONTENT_PREPARE_V1
                    && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1
            })
            .unwrap();
        service
            .set_workflow_automatic_execution_v1(&SetWorkflowAutomaticExecutionRequestV1 {
                step_id: content_step.step_id.clone(),
                enabled: false,
            })
            .unwrap();
        project
    };

    let mut runtime = NoProviderRuntime { pack };
    let mut control = CreatorRunControlServiceV1::for_writer(&session).unwrap();
    let outcome = control
        .start_or_resume_v1(
            &StartOrResumeCreatorRequestV1 {
                project_id: project.id,
                input: Some(CreatorInputV1::script(
                    "This input must not run while automatic Content is disabled.",
                )),
            },
            &mut runtime,
        )
        .unwrap();
    assert!(!outcome.progressed);
    assert_eq!(outcome.coordinator.action, CreatorRunActionV1::Review);
    assert_eq!(
        outcome.coordinator.blocking_step.as_deref(),
        Some(CREATOR_STEP_CONTENT_PREPARE_V1)
    );
}

#[test]
fn shared_start_resume_assembles_manual_flow_without_provider_calls() {
    let temp = tempfile::tempdir().unwrap();
    let data_root = temp.path().join("data-root");
    let workspace = Workspace::create(&data_root).unwrap();
    let session = WorkspaceSession::acquire(workspace, "phase18-run-manual").unwrap();
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let script = "Manual artifacts stay canonical while Start Resume only controls orchestration.";

    let project_id = {
        let mut service = ApplicationControlService::for_writer(&session).unwrap();
        let project = service
            .create_creator_project_v1("Shared Start Resume Manual", &pack)
            .unwrap()
            .data
            .project;
        let content = service
            .provide_manual_content_v1(&ManualContentRequestV1 {
                project_id: project.id.clone(),
                script: script.to_owned(),
                provenance: ManualResultProvenanceV1::manual_editor(),
            })
            .unwrap()
            .data;
        let segment_id = content.content.segments[0].id.clone();

        let (_, _, mut draft) = service
            .scene_plan_editor_v1(&ProjectIdRequestV1 {
                project_id: project.id.clone(),
            })
            .unwrap();
        draft.scenes[0].scene_type = "literal".to_owned();
        draft.scenes[0].purpose = "Prove shared Start Resume ownership".to_owned();
        let scene = service
            .provide_manual_scene_plan_v1(&ManualScenePlanRequestV1 {
                project_id: project.id.clone(),
                draft,
                provenance: ManualResultProvenanceV1::manual_editor(),
            })
            .unwrap()
            .data;

        let visual_path = temp.path().join("start-resume-visual.png");
        fs::write(&visual_path, png()).unwrap();
        service
            .provide_manual_visual_v1(&ManualVisualRequestV1 {
                project_id: project.id.clone(),
                scene_id: scene.scene_plan.scenes[0].id.clone(),
                source_path: visual_path,
                provenance: ManualResultProvenanceV1::local_file(),
                replace_existing: false,
            })
            .unwrap();

        let audio_path = temp.path().join("start-resume-audio.wav");
        fs::write(&audio_path, wav(1_500)).unwrap();
        service
            .provide_manual_voice_v1(&ManualVoiceRequestV1 {
                project_id: project.id.clone(),
                segment_id: segment_id.clone(),
                audio_path,
                timing: derive_manual_voice_timing_v1(&segment_id, script, 1_500).unwrap(),
                provenance: ManualResultProvenanceV1::local_file(),
                replace_existing: false,
            })
            .unwrap();
        project.id
    };

    let mut runtime = NoProviderRuntime { pack };
    let outcome = {
        let mut control = CreatorRunControlServiceV1::for_writer(&session).unwrap();
        control
            .start_or_resume_v1(
                &StartOrResumeCreatorRequestV1 {
                    project_id: project_id.clone(),
                    input: None,
                },
                &mut runtime,
            )
            .unwrap()
    };
    assert!(outcome.progressed);
    assert_eq!(outcome.coordinator.action, CreatorRunActionV1::Export);

    let mut service = ApplicationControlService::for_writer(&session).unwrap();
    let snapshot = service
        .project_status_v1(&ProjectIdRequestV1 {
            project_id: project_id.clone(),
        })
        .unwrap()
        .data;
    assert_eq!(
        snapshot
            .steps
            .iter()
            .find(|step| step.step == CREATOR_STEP_PRODUCTION_PACK_V1)
            .unwrap()
            .status,
        StepStatus::Succeeded
    );
    let json = serde_json::to_string(&outcome).unwrap();
    assert!(!json.contains(data_root.to_string_lossy().as_ref()));
    assert!(!json.contains("Bearer "));
    assert!(!json.contains("api_key"));
}

#[test]
fn read_only_shared_start_resume_rejects_before_runtime_resolution() {
    let temp = tempfile::tempdir().unwrap();
    let data_root = temp.path().join("data-root");
    let workspace = Workspace::create(&data_root).unwrap();
    let session = WorkspaceSession::acquire(workspace, "phase18-run-readonly").unwrap();
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let project_id = {
        let mut service = ApplicationControlService::for_writer(&session).unwrap();
        service
            .create_creator_project_v1("Read Only Start Resume", &pack)
            .unwrap()
            .data
            .project
            .id
    };
    session.release().unwrap();

    let workspace = Workspace::inspect(&data_root).unwrap();
    let mut control = CreatorRunControlServiceV1::for_read_only(&workspace).unwrap();
    let mut runtime = NoProviderRuntime { pack };
    let error = control
        .start_or_resume_v1(
            &StartOrResumeCreatorRequestV1 {
                project_id,
                input: None,
            },
            &mut runtime,
        )
        .unwrap_err();
    assert_eq!(error.code, ControlErrorCodeV1::ReadOnly);
}
