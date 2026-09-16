use std::fs;

use omnicreator_application::{
    AgentFanInRecoveryActionV1, AgentWorkStateV1, ApplicationControlService,
    ExternalResultBatchItemRequestV1, ExternalResultBatchRequestV1, ExternalVisualResultRequestV1,
    ExternalVoiceResultRequestV1, ManualContentRequestV1, ManualScenePlanRequestV1,
    ProjectIdRequestV1,
};
use omnicreator_core::{
    derive_manual_voice_timing_v1, initial_studio_pack_catalog_v1, ArtifactStore,
    ManualResultProvenanceV1, StateStore, Workspace, WorkspaceSession,
};

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
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
    out.extend(std::iter::repeat_n(5u8, data_size as usize));
    out
}

struct Seeded {
    project_id: String,
    scene_id: String,
    segment_id: String,
    narration: String,
}

fn seed(service: &mut ApplicationControlService<'_>) -> Seeded {
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let project = service
        .create_creator_project_v1("Phase 19 P5 fan-in", &pack)
        .unwrap()
        .data
        .project;
    let content = service
        .provide_manual_content_v1(&ManualContentRequestV1 {
            project_id: project.id.clone(),
            script: "A coordinator can restart without losing already verified worker outputs."
                .to_owned(),
            provenance: ManualResultProvenanceV1::manual_editor(),
        })
        .unwrap()
        .data;
    let segment = content.content.segments[0].clone();
    let request = ProjectIdRequestV1 {
        project_id: project.id.clone(),
    };
    let (_, _, mut draft) = service.scene_plan_editor_v1(&request).unwrap();
    draft.scenes[0].scene_type = "literal".to_owned();
    draft.scenes[0].purpose = "Show reconnect-safe fan-in".to_owned();
    draft.scenes[0].visual_ideas = vec!["editor reconnecting to a production board".to_owned()];
    draft.scenes[0].search_queries = vec!["video editor production board reconnect".to_owned()];
    let scene = service
        .provide_manual_scene_plan_v1(&ManualScenePlanRequestV1 {
            project_id: project.id.clone(),
            draft,
            provenance: ManualResultProvenanceV1::manual_editor(),
        })
        .unwrap()
        .data
        .scene_plan
        .scenes[0]
        .clone();
    Seeded {
        project_id: project.id,
        scene_id: scene.id,
        segment_id: segment.id,
        narration: segment.text,
    }
}

#[test]
fn fan_in_qa_survives_reconnect_and_data_root_move_then_exports() {
    let temp = tempfile::tempdir().unwrap();
    let data_root = temp.path().join("data-root");
    let workspace = Workspace::create(&data_root).unwrap();
    let session = WorkspaceSession::acquire(workspace, "phase19-p5-writer-a").unwrap();
    let mut service = ApplicationControlService::for_writer(&session).unwrap();
    let seeded = seed(&mut service);
    let project = ProjectIdRequestV1 {
        project_id: seeded.project_id.clone(),
    };

    let initial = service.agent_fan_in_qa_v1(&project).unwrap();
    assert_eq!(initial.visual_units.len(), 1);
    assert_eq!(initial.voice_units.len(), 1);
    assert_eq!(
        initial.visual_units[0].recovery_action,
        AgentFanInRecoveryActionV1::DispatchExternal
    );
    assert_eq!(
        initial.voice_units[0].recovery_action,
        AgentFanInRecoveryActionV1::DispatchExternal
    );
    assert!(!initial.fan_in_verified);
    assert!(!initial.production_pack_ready);

    let visual_request = service
        .prepare_external_visual_v1(&seeded.project_id, &seeded.scene_id)
        .unwrap()
        .data;
    let visual_path = temp.path().join("scene.png");
    fs::write(&visual_path, png(1280, 720)).unwrap();
    let visual_batch =
        ExternalResultBatchRequestV1::new(vec![ExternalResultBatchItemRequestV1::Visual {
            item_id: "visual-worker-result".to_owned(),
            result: Box::new(ExternalVisualResultRequestV1 {
                request: visual_request,
                source_path: visual_path,
                source_label: "phase19-p5-visual-worker".to_owned(),
                replace_existing: false,
            }),
        }]);
    let visual_result = service
        .provide_external_result_batch_v1(&visual_batch)
        .unwrap();
    assert_eq!(visual_result.committed, 1);

    let after_visual = service.agent_fan_in_qa_v1(&project).unwrap();
    assert_eq!(
        after_visual.visual_units[0].state,
        AgentWorkStateV1::Satisfied
    );
    assert_eq!(after_visual.voice_units[0].state, AgentWorkStateV1::Ready);
    let visual_artifact_ids = after_visual.visual_units[0].selected_artifact_ids.clone();
    assert!(!visual_artifact_ids.is_empty());

    drop(service);
    session.release().unwrap();

    let inspected = Workspace::inspect(&data_root).unwrap();
    let read_only = ApplicationControlService::for_read_only(&inspected).unwrap();
    let reconnected = read_only.agent_fan_in_qa_v1(&project).unwrap();
    assert!(reconnected.read_only);
    assert_eq!(
        reconnected.visual_units[0].state,
        AgentWorkStateV1::Satisfied
    );
    assert_eq!(reconnected.voice_units[0].state, AgentWorkStateV1::Ready);
    assert_eq!(
        reconnected.visual_units[0].selected_artifact_ids,
        visual_artifact_ids
    );
    drop(read_only);
    drop(inspected);

    let workspace = Workspace::open(&data_root).unwrap();
    let session = WorkspaceSession::acquire(workspace, "phase19-p5-writer-b").unwrap();
    let mut service = ApplicationControlService::for_writer(&session).unwrap();
    let voice_request = service
        .prepare_external_voice_v1(&seeded.project_id, &seeded.segment_id)
        .unwrap()
        .data;
    let audio_path = temp.path().join("segment.wav");
    fs::write(&audio_path, wav(1_500)).unwrap();
    let voice_batch =
        ExternalResultBatchRequestV1::new(vec![ExternalResultBatchItemRequestV1::Voice {
            item_id: "voice-worker-result".to_owned(),
            result: Box::new(ExternalVoiceResultRequestV1 {
                request: voice_request,
                audio_path,
                timing: derive_manual_voice_timing_v1(&seeded.segment_id, &seeded.narration, 1_500)
                    .unwrap(),
                source_label: "omnivoice-studio".to_owned(),
                replace_existing: false,
            }),
        }]);
    let voice_result = service
        .provide_external_result_batch_v1(&voice_batch)
        .unwrap();
    assert_eq!(voice_result.committed, 1);

    let fan_in = service.agent_fan_in_qa_v1(&project).unwrap();
    assert!(fan_in.fan_in_verified);
    assert!(fan_in.recovery_ready_for_rebuild);
    assert!(fan_in.production_pack_ready);
    assert_eq!(
        fan_in.suggested_action,
        AgentFanInRecoveryActionV1::AssembleProductionPack
    );

    let rebuilt = service
        .rebuild_and_export_production_v1(&project)
        .unwrap()
        .data;
    assert!(!rebuilt.export.artifacts.is_empty());
    let complete = service.agent_fan_in_qa_v1(&project).unwrap();
    assert!(complete.fan_in_verified);
    assert!(complete.production_pack_satisfied);

    drop(service);
    session.release().unwrap();

    let moved_root = temp.path().join("moved-data-root");
    fs::rename(&data_root, &moved_root).unwrap();
    let moved_workspace = Workspace::inspect(&moved_root).unwrap();
    let moved_service = ApplicationControlService::for_read_only(&moved_workspace).unwrap();
    let moved = moved_service.agent_fan_in_qa_v1(&project).unwrap();
    assert!(moved.fan_in_verified);
    assert!(moved.recovery_ready_for_rebuild);
    assert!(moved.production_pack_satisfied);
    assert_eq!(
        moved.visual_units[0].selected_artifact_ids,
        visual_artifact_ids
    );

    let serialized = serde_json::to_string(&moved).unwrap();
    assert!(!serialized.contains(data_root.to_string_lossy().as_ref()));
    assert!(!serialized.contains(moved_root.to_string_lossy().as_ref()));
    assert!(!serialized.contains("Bearer "));
    assert!(!serialized.contains("api_key"));
    assert!(!serialized.contains("credential"));
}

#[test]
fn fan_in_qa_surfaces_missing_verified_artifact_as_repair_work() {
    let temp = tempfile::tempdir().unwrap();
    let data_root = temp.path().join("data-root");
    let workspace = Workspace::create(&data_root).unwrap();
    let session = WorkspaceSession::acquire(workspace, "phase19-p5-recovery").unwrap();
    let mut service = ApplicationControlService::for_writer(&session).unwrap();
    let seeded = seed(&mut service);
    let project = ProjectIdRequestV1 {
        project_id: seeded.project_id.clone(),
    };

    let request = service
        .prepare_external_visual_v1(&seeded.project_id, &seeded.scene_id)
        .unwrap()
        .data;
    let visual_path = temp.path().join("scene.png");
    fs::write(&visual_path, png(1280, 720)).unwrap();
    let batch = ExternalResultBatchRequestV1::new(vec![ExternalResultBatchItemRequestV1::Visual {
        item_id: "visual-worker-result".to_owned(),
        result: Box::new(ExternalVisualResultRequestV1 {
            request,
            source_path: visual_path,
            source_label: "phase19-p5-visual-worker".to_owned(),
            replace_existing: false,
        }),
    }]);
    service.provide_external_result_batch_v1(&batch).unwrap();
    let healthy = service.agent_fan_in_qa_v1(&project).unwrap();
    let artifact_id = healthy.visual_units[0].selected_artifact_ids[0].clone();

    drop(service);
    session.release().unwrap();

    let workspace = Workspace::open(&data_root).unwrap();
    let store = StateStore::open(workspace.sqlite_path()).unwrap();
    let artifacts = ArtifactStore::new(&data_root).unwrap();
    let artifact = store.get_artifact(&artifact_id).unwrap();
    fs::remove_file(artifacts.resolve_artifact_path(&artifact).unwrap()).unwrap();
    drop(store);
    drop(workspace);

    let workspace = Workspace::inspect(&data_root).unwrap();
    let service = ApplicationControlService::for_read_only(&workspace).unwrap();
    let qa = service.agent_fan_in_qa_v1(&project).unwrap();
    assert_eq!(qa.visual_units[0].state, AgentWorkStateV1::NeedsReview);
    assert_eq!(
        qa.visual_units[0].recovery_action,
        AgentFanInRecoveryActionV1::RepairVisual
    );
    assert!(qa.needs_review_work_ids.contains(&qa.visual_units[0].work_id));
    assert_eq!(qa.unhealthy_canonical_units, vec![seeded.scene_id]);
    assert!(!qa.recovery_ready_for_rebuild);
    assert!(!qa.fan_in_verified);
}
