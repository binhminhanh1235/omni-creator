use std::fs;

use omnicreator_core::{
    compile_creator_workflow_plan_v1, derive_manual_voice_timing_v1,
    initial_studio_pack_catalog_v1, inspect_creator_production_recovery_v1,
    materialize_creator_workflow_plan_v1, provide_manual_creator_content_v1,
    provide_manual_creator_scene_plan_v1, provide_manual_creator_visual_file_v1,
    provide_manual_creator_voice_bundle_v1, rebuild_and_export_creator_production_v1,
    ArtifactStore, ManualCreatorVoiceRequestV1, ManualResultProvenanceV1, ManualScenePlanDraftV1,
    StateStore, Workspace,
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

#[test]
fn all_external_services_unavailable_fully_manual_flow_exports_to_resolve() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data-root")).unwrap();
    let mut store = StateStore::open(workspace.sqlite_path()).unwrap();
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();

    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let project = store
        .create_project_with_studio_pack("Fully manual Phase 16", Some(&pack.id))
        .unwrap();
    let plan = compile_creator_workflow_plan_v1(&project, &pack).unwrap();
    materialize_creator_workflow_plan_v1(&store, &plan).unwrap();

    let script = "A fully manual production can keep moving without any external service.";
    let content = provide_manual_creator_content_v1(
        &mut store,
        &artifacts,
        &project.id,
        script,
        ManualResultProvenanceV1::manual_editor(),
    )
    .unwrap();
    let segment_id = content.content.segments[0].id.clone();

    let mut scene_draft = ManualScenePlanDraftV1::for_content_v1(&content.content).unwrap();
    scene_draft.scenes[0].scene_type = "literal".to_owned();
    scene_draft.scenes[0].purpose = "Fully manual visual".to_owned();
    let scene = provide_manual_creator_scene_plan_v1(
        &mut store,
        &artifacts,
        &project.id,
        &scene_draft,
        ManualResultProvenanceV1::manual_editor(),
    )
    .unwrap();
    let scene_id = scene.scene_plan.scenes[0].id.clone();

    let visual_path = temp.path().join("manual-visual.png");
    fs::write(&visual_path, png()).unwrap();
    let visual = provide_manual_creator_visual_file_v1(
        &mut store,
        &artifacts,
        &project.id,
        &scene_id,
        &visual_path,
        ManualResultProvenanceV1::local_file(),
        false,
    )
    .unwrap();
    assert!(artifacts.verify_artifact(&visual.artifact).unwrap());

    let audio_path = temp.path().join("manual-audio.wav");
    fs::write(&audio_path, wav(1_500)).unwrap();
    let timing = derive_manual_voice_timing_v1(&segment_id, script, 1_500).unwrap();
    let voice = provide_manual_creator_voice_bundle_v1(
        &mut store,
        &artifacts,
        ManualCreatorVoiceRequestV1 {
            project_id: &project.id,
            segment_id: &segment_id,
            audio_path: &audio_path,
            timing,
            provenance: ManualResultProvenanceV1::local_file(),
            replace_existing: false,
        },
    )
    .unwrap();
    assert!(artifacts.verify_artifact(&voice.audio).unwrap());
    assert!(artifacts.verify_artifact(&voice.timing_artifact).unwrap());

    let recovery = inspect_creator_production_recovery_v1(&store, &artifacts, &project.id).unwrap();
    assert!(recovery.ready_for_rebuild);
    assert!(recovery.items.iter().all(|item| item.detail.is_none()));

    let outcome =
        rebuild_and_export_creator_production_v1(&mut store, &artifacts, &project.id).unwrap();
    assert!(!outcome.assembly.production_pack.tracks.is_empty());
    assert!(!outcome.export.artifacts.is_empty());
    assert!(outcome
        .export
        .artifacts
        .iter()
        .all(|artifact| artifacts.verify_artifact(artifact).unwrap()));

    let pack_json = serde_json::to_string(&outcome.assembly.production_pack).unwrap();
    assert!(!pack_json.contains(workspace.data_root().to_string_lossy().as_ref()));

    // No provider execution is needed anywhere in this test: Script -> ScenePlan -> Visual ->
    // Audio + Timing -> ProductionPack -> Resolve export all use canonical manual paths.
}
