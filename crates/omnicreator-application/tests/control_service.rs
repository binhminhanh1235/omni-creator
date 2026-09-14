use std::fs;

use omnicreator_application::{
    ApplicationControlService, ControlErrorCodeV1, ManualContentRequestV1,
    ManualScenePlanRequestV1, ManualVisualRequestV1, ManualVoiceRequestV1, ProjectIdRequestV1,
    RenameProjectRequestV1, SetWorkflowAutomaticExecutionRequestV1,
};
use omnicreator_core::{
    derive_manual_voice_timing_v1, initial_studio_pack_catalog_v1, ManualResultProvenanceV1,
    StepStatus, Workspace, WorkspaceSession, CREATOR_STEP_CONTENT_PREPARE_V1,
    CREATOR_WORKFLOW_UNIT_PROJECT_V1,
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
    out.extend(std::iter::repeat_n(7u8, data_size as usize));
    out
}

#[test]
fn application_service_drives_fully_manual_flow_to_resolve_export() {
    let temp = tempfile::tempdir().unwrap();
    let data_root = temp.path().join("data-root");
    let workspace = Workspace::create(&data_root).unwrap();
    let session = WorkspaceSession::acquire(workspace, "phase18-test-device").unwrap();
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let mut service = ApplicationControlService::for_writer(&session).unwrap();

    let project = service
        .create_creator_project_v1("Phase 18 application manual flow", &pack)
        .unwrap()
        .data
        .project;
    let script = "A transport-neutral control service must preserve canonical workflow truth.";
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
    draft.scenes[0].purpose = "Prove shared application control".to_owned();
    let scene = service
        .provide_manual_scene_plan_v1(&ManualScenePlanRequestV1 {
            project_id: project.id.clone(),
            draft,
            provenance: ManualResultProvenanceV1::manual_editor(),
        })
        .unwrap()
        .data;
    let scene_id = scene.scene_plan.scenes[0].id.clone();

    let visual_path = temp.path().join("manual-visual.png");
    fs::write(&visual_path, png()).unwrap();
    service
        .provide_manual_visual_v1(&ManualVisualRequestV1 {
            project_id: project.id.clone(),
            scene_id,
            source_path: visual_path,
            provenance: ManualResultProvenanceV1::local_file(),
            replace_existing: false,
        })
        .unwrap();

    let audio_path = temp.path().join("manual-audio.wav");
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

    let recovery = service
        .production_recovery_v1(&ProjectIdRequestV1 {
            project_id: project.id.clone(),
        })
        .unwrap()
        .data;
    assert!(recovery.recovery.ready_for_rebuild);
    assert!(recovery
        .recovery
        .items
        .iter()
        .all(|item| item.detail.is_none()));

    let exported = service
        .rebuild_and_export_production_v1(&ProjectIdRequestV1 {
            project_id: project.id.clone(),
        })
        .unwrap()
        .data;
    assert!(!exported.assembly.production_pack.tracks.is_empty());
    assert!(!exported.export.artifacts.is_empty());

    let snapshot = service
        .project_status_v1(&ProjectIdRequestV1 {
            project_id: project.id,
        })
        .unwrap()
        .data;
    assert!(snapshot.jobs.iter().any(|job| {
        job.status == StepStatus::Succeeded
            && job.selected_attempt.is_some()
            && job.selected_artifact.is_some()
    }));
    let json = serde_json::to_string(&snapshot).unwrap();
    assert!(!json.contains(data_root.to_string_lossy().as_ref()));
    assert!(!json.contains("Bearer "));
    assert!(!json.contains("api_key"));
}

#[test]
fn read_only_application_session_rejects_mutation_before_core_write() {
    let temp = tempfile::tempdir().unwrap();
    let data_root = temp.path().join("data-root");
    let workspace = Workspace::create(&data_root).unwrap();
    let session = WorkspaceSession::acquire(workspace, "phase18-writer").unwrap();
    let project_id = {
        let mut service = ApplicationControlService::for_writer(&session).unwrap();
        service
            .create_project_v1(&omnicreator_application::CreateProjectRequestV1 {
                title: "Read only invariant".to_owned(),
            })
            .unwrap()
            .data
            .project
            .id
    };
    session.release().unwrap();

    let workspace = Workspace::inspect(&data_root).unwrap();
    let mut read_only = ApplicationControlService::for_read_only(&workspace).unwrap();
    let inspection = read_only.workspace_status_v1().unwrap().data;
    assert!(inspection.read_only);

    let error = read_only
        .rename_project_v1(&RenameProjectRequestV1 {
            project_id,
            title: "Must not persist".to_owned(),
        })
        .unwrap_err();
    assert_eq!(error.code, ControlErrorCodeV1::ReadOnly);
}

#[test]
fn workflow_off_is_policy_only_and_manual_takeover_can_satisfy_content() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data-root")).unwrap();
    let session = WorkspaceSession::acquire(workspace, "phase18-policy-test").unwrap();
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let mut service = ApplicationControlService::for_writer(&session).unwrap();
    let project = service
        .create_creator_project_v1("Workflow policy", &pack)
        .unwrap()
        .data
        .project;

    let before = service
        .project_status_v1(&ProjectIdRequestV1 {
            project_id: project.id.clone(),
        })
        .unwrap()
        .data;
    let content_step = before
        .steps
        .iter()
        .find(|step| {
            step.step == CREATOR_STEP_CONTENT_PREPARE_V1
                && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1
        })
        .unwrap()
        .clone();

    service
        .set_workflow_automatic_execution_v1(&SetWorkflowAutomaticExecutionRequestV1 {
            step_id: content_step.step_id.clone(),
            enabled: false,
        })
        .unwrap();
    let policy = service
        .workflow_execution_policies_v1(&ProjectIdRequestV1 {
            project_id: project.id.clone(),
        })
        .unwrap()
        .data
        .into_iter()
        .find(|policy| policy.step_id == content_step.step_id)
        .unwrap();
    assert!(!policy.automatic_execution_enabled);

    let after_off = service
        .project_status_v1(&ProjectIdRequestV1 {
            project_id: project.id.clone(),
        })
        .unwrap()
        .data;
    let content_after_off = after_off
        .steps
        .iter()
        .find(|step| step.step_id == content_step.step_id)
        .unwrap();
    assert_ne!(content_after_off.status, StepStatus::Skipped);
    assert_ne!(content_after_off.status, StepStatus::Succeeded);

    service
        .provide_manual_content_v1(&ManualContentRequestV1 {
            project_id: project.id.clone(),
            script: "Manual content remains the canonical result while automatic execution is off."
                .to_owned(),
            provenance: ManualResultProvenanceV1::manual_editor(),
        })
        .unwrap();
    let after_manual = service
        .project_status_v1(&ProjectIdRequestV1 {
            project_id: project.id,
        })
        .unwrap()
        .data;
    assert_eq!(
        after_manual
            .steps
            .iter()
            .find(|step| step.step_id == content_step.step_id)
            .unwrap()
            .status,
        StepStatus::Succeeded
    );
}
