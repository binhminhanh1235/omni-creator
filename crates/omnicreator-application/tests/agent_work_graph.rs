use std::fs;

use omnicreator_application::{
    AgentExternalWorkDescriptorV1, AgentWorkGraphV1, AgentWorkKindV1, AgentWorkStateV1,
    ApplicationControlService, ManualContentRequestV1, ManualScenePlanRequestV1,
    ManualVisualRequestV1, ManualVoiceRequestV1, ProjectIdRequestV1,
};
use omnicreator_core::{
    derive_manual_voice_timing_v1, initial_studio_pack_catalog_v1, ManualResultProvenanceV1,
    Workspace, WorkspaceSession, CREATOR_WORKFLOW_UNIT_PROJECT_V1,
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

fn project_item(graph: &AgentWorkGraphV1, kind: AgentWorkKindV1) -> &omnicreator_application::AgentWorkItemV1 {
    graph
        .items
        .iter()
        .find(|item| item.kind == kind && item.canonical_unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1)
        .unwrap()
}

#[test]
fn agent_work_graph_exposes_parallel_voice_then_visual_without_shadow_state() {
    let temp = tempfile::tempdir().unwrap();
    let data_root = temp.path().join("data-root");
    let workspace = Workspace::create(&data_root).unwrap();
    let session = WorkspaceSession::acquire(workspace, "phase19-p0-writer").unwrap();
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let mut service = ApplicationControlService::for_writer(&session).unwrap();
    let project = service
        .create_creator_project_v1("Phase 19 parallel graph", &pack)
        .unwrap()
        .data
        .project;
    let request = ProjectIdRequestV1 {
        project_id: project.id.clone(),
    };

    let initial = service.agent_work_graph_v1(&request).unwrap();
    assert!(!initial.read_only);
    assert_eq!(
        project_item(&initial, AgentWorkKindV1::Content).state,
        AgentWorkStateV1::Ready
    );
    assert_eq!(
        project_item(&initial, AgentWorkKindV1::ScenePlan).state,
        AgentWorkStateV1::Blocked
    );
    assert!(!initial
        .items
        .iter()
        .any(|item| matches!(item.kind, AgentWorkKindV1::Visual | AgentWorkKindV1::Voice)));

    let script = "A canonical work graph lets independent creator workers execute safely in parallel.";
    let content = service
        .provide_manual_content_v1(&ManualContentRequestV1 {
            project_id: project.id.clone(),
            script: script.to_owned(),
            provenance: ManualResultProvenanceV1::manual_editor(),
        })
        .unwrap()
        .data;
    let segment = content.content.segments[0].clone();

    let after_content = service.agent_work_graph_v1(&request).unwrap();
    assert_eq!(
        project_item(&after_content, AgentWorkKindV1::Content).state,
        AgentWorkStateV1::Satisfied
    );
    assert_eq!(
        project_item(&after_content, AgentWorkKindV1::ScenePlan).state,
        AgentWorkStateV1::Ready
    );
    let voice = after_content
        .items
        .iter()
        .find(|item| item.kind == AgentWorkKindV1::Voice)
        .unwrap();
    assert_eq!(voice.state, AgentWorkStateV1::Ready);
    assert!(voice.input_sha256.as_ref().is_some_and(|value| !value.is_empty()));
    assert!(matches!(
        &voice.external,
        Some(AgentExternalWorkDescriptorV1::Voice { .. })
    ));
    assert!(!after_content
        .items
        .iter()
        .any(|item| item.kind == AgentWorkKindV1::Visual));

    let (_, _, mut draft) = service.scene_plan_editor_v1(&request).unwrap();
    draft.scenes[0].scene_type = "literal".to_owned();
    draft.scenes[0].purpose = "Show independent worker fan-out".to_owned();
    draft.scenes[0].visual_ideas = vec!["editorial creator workstation".to_owned()];
    draft.scenes[0].search_queries = vec!["creator workstation video editing".to_owned()];
    let scene_plan = service
        .provide_manual_scene_plan_v1(&ManualScenePlanRequestV1 {
            project_id: project.id.clone(),
            draft,
            provenance: ManualResultProvenanceV1::manual_editor(),
        })
        .unwrap()
        .data;
    let scene_id = scene_plan.scene_plan.scenes[0].id.clone();

    let fan_out = service.agent_work_graph_v1(&request).unwrap();
    assert_eq!(
        project_item(&fan_out, AgentWorkKindV1::ScenePlan).state,
        AgentWorkStateV1::Satisfied
    );
    let visual = fan_out
        .items
        .iter()
        .find(|item| item.kind == AgentWorkKindV1::Visual)
        .unwrap();
    assert_eq!(visual.state, AgentWorkStateV1::Ready);
    assert!(visual.input_sha256.as_ref().is_some_and(|value| !value.is_empty()));
    assert!(matches!(
        &visual.external,
        Some(AgentExternalWorkDescriptorV1::Visual { .. })
    ));
    assert!(fan_out.ready_work_ids.iter().any(|id| id.starts_with("voice:")));
    assert!(fan_out.ready_work_ids.iter().any(|id| id.starts_with("visual:")));

    let visual_path = temp.path().join("scene.png");
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

    let audio_path = temp.path().join("segment.wav");
    fs::write(&audio_path, wav(1_500)).unwrap();
    service
        .provide_manual_voice_v1(&ManualVoiceRequestV1 {
            project_id: project.id.clone(),
            segment_id: segment.id.clone(),
            audio_path,
            timing: derive_manual_voice_timing_v1(&segment.id, &segment.text, 1_500).unwrap(),
            provenance: ManualResultProvenanceV1::local_file(),
            replace_existing: false,
        })
        .unwrap();

    let fan_in = service.agent_work_graph_v1(&request).unwrap();
    assert!(fan_in
        .items
        .iter()
        .filter(|item| matches!(item.kind, AgentWorkKindV1::Visual | AgentWorkKindV1::Voice))
        .all(|item| item.state == AgentWorkStateV1::Satisfied && item.external.is_none()));
    assert_eq!(
        project_item(&fan_in, AgentWorkKindV1::ProductionPack).state,
        AgentWorkStateV1::Ready
    );

    let json = serde_json::to_string(&fan_in).unwrap();
    assert!(!json.contains(data_root.to_string_lossy().as_ref()));
    assert!(!json.contains("Bearer "));
    assert!(!json.contains("api_key"));
    assert!(!json.contains("credential"));

    drop(service);
    session.release().unwrap();
    let workspace = Workspace::inspect(&data_root).unwrap();
    let read_only = ApplicationControlService::for_read_only(&workspace).unwrap();
    let read_only_graph = read_only.agent_work_graph_v1(&request).unwrap();
    assert!(read_only_graph.read_only);
    assert_eq!(read_only_graph.satisfied_work_ids, fan_in.satisfied_work_ids);
}
