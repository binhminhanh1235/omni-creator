use std::{fs, path::Path};

use omnicreator_application::{
    ApplicationControlService, ControlErrorCodeV1, ExternalResultBatchItemRequestV1,
    ExternalResultBatchItemStatusV1, ExternalResultBatchRequestV1, ExternalVisualResultRequestV1,
    ExternalVoiceResultRequestV1, ManualContentRequestV1, ManualScenePlanRequestV1,
    ProjectIdRequestV1,
};
use omnicreator_core::{
    derive_manual_voice_timing_v1, initial_studio_pack_catalog_v1,
    ExternalGeneratedVisualRequestV1, ExternalVoiceRequestV1, ManualResultProvenanceV1, Workspace,
    WorkspaceSession,
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
    out.extend(std::iter::repeat_n(7u8, data_size as usize));
    out
}

struct SeededProject {
    project_id: String,
    scene_id: String,
    segment_id: String,
    narration: String,
    visual_request: ExternalGeneratedVisualRequestV1,
    voice_request: ExternalVoiceRequestV1,
}

fn seed_project(service: &mut ApplicationControlService<'_>, title: &str) -> SeededProject {
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let project = service
        .create_creator_project_v1(title, &pack)
        .unwrap()
        .data
        .project;
    let content = service
        .provide_manual_content_v1(&ManualContentRequestV1 {
            project_id: project.id.clone(),
            script: "Parallel workers must return only results that still match canonical creator input."
                .to_owned(),
            provenance: ManualResultProvenanceV1::manual_editor(),
        })
        .unwrap()
        .data;
    let segment = content.content.segments[0].clone();
    let project_request = ProjectIdRequestV1 {
        project_id: project.id.clone(),
    };
    let (_, _, mut draft) = service.scene_plan_editor_v1(&project_request).unwrap();
    draft.scenes[0].scene_type = "literal".to_owned();
    draft.scenes[0].purpose = "Show serialized canonical commit".to_owned();
    draft.scenes[0].visual_ideas = vec!["video editor reviewing a storyboard".to_owned()];
    draft.scenes[0].search_queries = vec!["video editor storyboard workstation".to_owned()];
    let scene_plan = service
        .provide_manual_scene_plan_v1(&ManualScenePlanRequestV1 {
            project_id: project.id.clone(),
            draft,
            provenance: ManualResultProvenanceV1::manual_editor(),
        })
        .unwrap()
        .data;
    let scene_id = scene_plan.scene_plan.scenes[0].id.clone();
    let visual_request = service
        .prepare_external_visual_v1(&project.id, &scene_id)
        .unwrap()
        .data;
    let voice_request = service
        .prepare_external_voice_v1(&project.id, &segment.id)
        .unwrap()
        .data;

    SeededProject {
        project_id: project.id,
        scene_id,
        segment_id: segment.id,
        narration: segment.text,
        visual_request,
        voice_request,
    }
}

fn visual_item(
    item_id: &str,
    request: ExternalGeneratedVisualRequestV1,
    source_path: &Path,
    replace_existing: bool,
) -> ExternalResultBatchItemRequestV1 {
    ExternalResultBatchItemRequestV1::Visual {
        item_id: item_id.to_owned(),
        result: ExternalVisualResultRequestV1 {
            request,
            source_path: source_path.to_path_buf(),
            source_label: "phase19-test-visual-worker".to_owned(),
            replace_existing,
        },
    }
}

fn voice_item(
    item_id: &str,
    request: ExternalVoiceRequestV1,
    audio_path: &Path,
    segment_id: &str,
    narration: &str,
    replace_existing: bool,
) -> ExternalResultBatchItemRequestV1 {
    ExternalResultBatchItemRequestV1::Voice {
        item_id: item_id.to_owned(),
        result: ExternalVoiceResultRequestV1 {
            request,
            audio_path: audio_path.to_path_buf(),
            timing: derive_manual_voice_timing_v1(segment_id, narration, 1_500).unwrap(),
            source_label: "phase19-test-voice-worker".to_owned(),
            replace_existing,
        },
    }
}

#[test]
fn batch_commits_mixed_results_and_reconnect_delivery_is_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let data_root = temp.path().join("data-root");
    let workspace = Workspace::create(&data_root).unwrap();
    let session = WorkspaceSession::acquire(workspace, "phase19-p1-writer").unwrap();
    let mut service = ApplicationControlService::for_writer(&session).unwrap();
    let seeded = seed_project(&mut service, "Phase 19 P1 idempotency");

    let visual_path = temp.path().join("scene.png");
    let audio_path = temp.path().join("segment.wav");
    fs::write(&visual_path, png(1280, 720)).unwrap();
    fs::write(&audio_path, wav(1_500)).unwrap();

    let batch = ExternalResultBatchRequestV1::new(vec![
        visual_item(
            "visual-1",
            seeded.visual_request.clone(),
            &visual_path,
            false,
        ),
        voice_item(
            "voice-1",
            seeded.voice_request.clone(),
            &audio_path,
            &seeded.segment_id,
            &seeded.narration,
            false,
        ),
    ]);
    let first = service.provide_external_result_batch_v1(&batch).unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.committed, 2);
    assert_eq!(first.reused, 0);
    assert_eq!(first.failed, 0);
    assert!(first
        .items
        .iter()
        .all(|item| item.status == ExternalResultBatchItemStatusV1::Committed));

    let second = service.provide_external_result_batch_v1(&batch).unwrap();
    assert_eq!(second.committed, 0);
    assert_eq!(second.reused, 2);
    assert_eq!(second.failed, 0);
    assert!(second
        .items
        .iter()
        .all(|item| item.status == ExternalResultBatchItemStatusV1::Reused));
    assert_eq!(
        first
            .items
            .iter()
            .map(|item| item.artifact_ids.clone())
            .collect::<Vec<_>>(),
        second
            .items
            .iter()
            .map(|item| item.artifact_ids.clone())
            .collect::<Vec<_>>()
    );

    let json = serde_json::to_string(&second).unwrap();
    assert!(!json.contains(temp.path().to_string_lossy().as_ref()));
    assert!(!json.contains(data_root.to_string_lossy().as_ref()));

    drop(service);
    session.release().unwrap();
    let workspace = Workspace::inspect(&data_root).unwrap();
    let mut read_only = ApplicationControlService::for_read_only(&workspace).unwrap();
    let error = read_only
        .provide_external_result_batch_v1(&batch)
        .unwrap_err();
    assert_eq!(error.code, ControlErrorCodeV1::ReadOnly);
}

#[test]
fn batch_reports_partial_failure_and_requires_explicit_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let data_root = temp.path().join("data-root");
    let workspace = Workspace::create(&data_root).unwrap();
    let session = WorkspaceSession::acquire(workspace, "phase19-p1-partial").unwrap();
    let mut service = ApplicationControlService::for_writer(&session).unwrap();
    let seeded = seed_project(&mut service, "Phase 19 P1 partial batch");

    let visual_path = temp.path().join("scene-a.png");
    let audio_path = temp.path().join("segment.wav");
    fs::write(&visual_path, png(1280, 720)).unwrap();
    fs::write(&audio_path, wav(1_500)).unwrap();

    let partial = ExternalResultBatchRequestV1::new(vec![
        visual_item(
            "visual-ok",
            seeded.visual_request.clone(),
            &visual_path,
            false,
        ),
        voice_item(
            "voice-bad-timing",
            seeded.voice_request.clone(),
            &audio_path,
            "wrong-segment",
            &seeded.narration,
            false,
        ),
    ]);
    let outcome = service.provide_external_result_batch_v1(&partial).unwrap();
    assert_eq!(outcome.committed, 1);
    assert_eq!(outcome.failed, 1);
    assert_eq!(
        outcome.items[0].status,
        ExternalResultBatchItemStatusV1::Committed
    );
    assert_eq!(
        outcome.items[1].status,
        ExternalResultBatchItemStatusV1::Failed
    );
    assert!(outcome.items[1].error.is_some());

    let replacement_path = temp.path().join("scene-b.png");
    fs::write(&replacement_path, png(1024, 1024)).unwrap();
    let denied = ExternalResultBatchRequestV1::new(vec![visual_item(
        "visual-replacement-denied",
        seeded.visual_request.clone(),
        &replacement_path,
        false,
    )]);
    let denied = service.provide_external_result_batch_v1(&denied).unwrap();
    assert_eq!(denied.failed, 1);
    assert_eq!(
        denied.items[0].error.as_ref().unwrap().code,
        ControlErrorCodeV1::InvalidTransition
    );

    let allowed = ExternalResultBatchRequestV1::new(vec![visual_item(
        "visual-replacement-allowed",
        seeded.visual_request,
        &replacement_path,
        true,
    )]);
    let allowed = service.provide_external_result_batch_v1(&allowed).unwrap();
    assert_eq!(allowed.committed, 1);
    assert_eq!(allowed.failed, 0);
}

#[test]
fn stale_visual_does_not_block_current_voice_result_in_same_batch() {
    let temp = tempfile::tempdir().unwrap();
    let data_root = temp.path().join("data-root");
    let workspace = Workspace::create(&data_root).unwrap();
    let session = WorkspaceSession::acquire(workspace, "phase19-p1-stale").unwrap();
    let mut service = ApplicationControlService::for_writer(&session).unwrap();
    let seeded = seed_project(&mut service, "Phase 19 P1 stale guard");

    let project_request = ProjectIdRequestV1 {
        project_id: seeded.project_id.clone(),
    };
    let (_, _, mut draft) = service.scene_plan_editor_v1(&project_request).unwrap();
    draft.scenes[0].purpose = "ScenePlan changed after worker dispatch".to_owned();
    service
        .provide_manual_scene_plan_v1(&ManualScenePlanRequestV1 {
            project_id: seeded.project_id.clone(),
            draft,
            provenance: ManualResultProvenanceV1::manual_editor(),
        })
        .unwrap();

    let visual_path = temp.path().join("stale-scene.png");
    let audio_path = temp.path().join("current-segment.wav");
    fs::write(&visual_path, png(1280, 720)).unwrap();
    fs::write(&audio_path, wav(1_500)).unwrap();
    let batch = ExternalResultBatchRequestV1::new(vec![
        visual_item("stale-visual", seeded.visual_request, &visual_path, false),
        voice_item(
            "current-voice",
            seeded.voice_request,
            &audio_path,
            &seeded.segment_id,
            &seeded.narration,
            false,
        ),
    ]);
    let outcome = service.provide_external_result_batch_v1(&batch).unwrap();
    assert_eq!(outcome.committed, 1);
    assert_eq!(outcome.failed, 1);
    assert_eq!(
        outcome.items[0].status,
        ExternalResultBatchItemStatusV1::Failed
    );
    assert_eq!(
        outcome.items[0].error.as_ref().unwrap().code,
        ControlErrorCodeV1::StaleInput
    );
    assert_eq!(
        outcome.items[1].status,
        ExternalResultBatchItemStatusV1::Committed
    );
    assert_eq!(outcome.items[0].canonical_unit, seeded.scene_id);
}
