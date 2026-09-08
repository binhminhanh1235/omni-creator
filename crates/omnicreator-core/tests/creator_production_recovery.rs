use std::fs;

use omnicreator_core::{
    assemble_creator_production_pack_v1, compile_creator_workflow_plan_v1,
    derive_manual_voice_timing_v1, initial_studio_pack_catalog_v1,
    inspect_creator_production_recovery_v1, materialize_creator_workflow_plan_v1,
    provide_manual_creator_content_v1, provide_manual_creator_scene_plan_v1,
    provide_manual_creator_visual_file_v1, provide_manual_creator_voice_bundle_v1,
    rebuild_and_export_creator_production_v1, repair_creator_production_audio_v1,
    repair_creator_production_timing_v1, repair_creator_production_visual_v1, ArtifactStore,
    CreatorProductionPackOptionsV1, ManualCreatorVoiceRequestV1, ManualResultProvenanceV1,
    ManualScenePlanDraftV1, ProductionRecoveryArtifactKindV1, ProductionRecoveryArtifactStateV1,
    StateStore, Workspace,
};

struct Fixture {
    temp: tempfile::TempDir,
    workspace: Workspace,
    store: StateStore,
    project_id: String,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let store = StateStore::open(workspace.sqlite_path()).unwrap();
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let project = store
        .create_project_with_studio_pack("P5 recovery", Some(&pack.id))
        .unwrap();
    let plan = compile_creator_workflow_plan_v1(&project, &pack).unwrap();
    materialize_creator_workflow_plan_v1(&store, &plan).unwrap();
    Fixture {
        temp,
        workspace,
        store,
        project_id: project.id,
    }
}

fn png(marker: u8) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&1280u32.to_be_bytes());
    bytes.extend_from_slice(&720u32.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
    bytes.extend_from_slice(&[0, 0, 0, 0, marker]);
    bytes
}

fn wav(duration_ms: u64, marker: u8) -> Vec<u8> {
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
    out.extend(std::iter::repeat(marker).take(data_size as usize));
    out
}

fn prepare(fx: &mut Fixture) -> (String, String) {
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();
    let content = provide_manual_creator_content_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        "One recoverable production segment.",
        ManualResultProvenanceV1::manual_editor(),
    )
    .unwrap();
    let mut draft = ManualScenePlanDraftV1::for_content_v1(&content.content).unwrap();
    draft.scenes[0].scene_type = "literal".to_owned();
    draft.scenes[0].purpose = "Recovery scene".to_owned();
    let scene = provide_manual_creator_scene_plan_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        &draft,
        ManualResultProvenanceV1::manual_editor(),
    )
    .unwrap();
    let scene_id = scene.scene_plan.scenes[0].id.clone();
    let segment_id = content.content.segments[0].id.clone();

    let visual = fx.temp.path().join("visual.png");
    fs::write(&visual, png(1)).unwrap();
    provide_manual_creator_visual_file_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        &scene_id,
        &visual,
        ManualResultProvenanceV1::local_file(),
        false,
    )
    .unwrap();

    let audio = fx.temp.path().join("voice.wav");
    fs::write(&audio, wav(1_000, 2)).unwrap();
    let timing =
        derive_manual_voice_timing_v1(&segment_id, "One recoverable production segment.", 1_000)
            .unwrap();
    provide_manual_creator_voice_bundle_v1(
        &mut fx.store,
        &artifacts,
        ManualCreatorVoiceRequestV1 {
            project_id: &fx.project_id,
            segment_id: &segment_id,
            audio_path: &audio,
            timing,
            provenance: ManualResultProvenanceV1::local_file(),
            replace_existing: false,
        },
    )
    .unwrap();

    assemble_creator_production_pack_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        &CreatorProductionPackOptionsV1::default(),
    )
    .unwrap();
    (scene_id, segment_id)
}

fn item<'a>(
    view: &'a omnicreator_core::ProductionRecoveryViewV1,
    kind: ProductionRecoveryArtifactKindV1,
) -> &'a omnicreator_core::ProductionRecoveryItemV1 {
    view.items.iter().find(|item| item.kind == kind).unwrap()
}

#[test]
fn missing_visual_relinks_through_canonical_replacement_and_rebuilds_export() {
    let mut fx = fixture();
    let (scene_id, _) = prepare(&mut fx);
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();
    let before =
        inspect_creator_production_recovery_v1(&fx.store, &artifacts, &fx.project_id).unwrap();
    assert!(before.ready_for_rebuild);
    let visual = item(&before, ProductionRecoveryArtifactKindV1::Visual);
    let old_id = visual.artifact_id.clone().unwrap();
    let old_artifact = fx.store.get_artifact(&old_id).unwrap();
    fs::remove_file(artifacts.resolve_artifact_path(&old_artifact).unwrap()).unwrap();

    let broken =
        inspect_creator_production_recovery_v1(&fx.store, &artifacts, &fx.project_id).unwrap();
    assert_eq!(
        item(&broken, ProductionRecoveryArtifactKindV1::Visual).state,
        ProductionRecoveryArtifactStateV1::Missing
    );
    assert!(!broken.ready_for_rebuild);

    let replacement = fx.temp.path().join("visual-relinked.png");
    fs::write(&replacement, png(9)).unwrap();
    let repaired = repair_creator_production_visual_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        &scene_id,
        &replacement,
    )
    .unwrap();
    assert_ne!(repaired.artifact.artifact_id, old_id);
    assert!(artifacts.verify_artifact(&repaired.artifact).unwrap());

    let rebuilt =
        rebuild_and_export_creator_production_v1(&mut fx.store, &artifacts, &fx.project_id)
            .unwrap();
    assert!(rebuilt
        .assembly
        .production_pack
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .any(|clip| clip.artifact_id == repaired.artifact.artifact_id));
    assert!(rebuilt
        .export
        .artifacts
        .iter()
        .all(|artifact| artifacts.verify_artifact(artifact).unwrap()));
    let json = serde_json::to_string(&rebuilt.assembly.production_pack).unwrap();
    assert!(!json.contains(fx.workspace.data_root().to_string_lossy().as_ref()));
}

#[test]
fn audio_then_timing_recovery_preserves_segment_identity_and_survives_data_root_move() {
    let mut fx = fixture();
    let (_, segment_id) = prepare(&mut fx);
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();
    let initial =
        inspect_creator_production_recovery_v1(&fx.store, &artifacts, &fx.project_id).unwrap();
    let audio_id = item(&initial, ProductionRecoveryArtifactKindV1::Audio)
        .artifact_id
        .clone()
        .unwrap();
    let audio_artifact = fx.store.get_artifact(&audio_id).unwrap();
    fs::remove_file(artifacts.resolve_artifact_path(&audio_artifact).unwrap()).unwrap();

    let broken_audio =
        inspect_creator_production_recovery_v1(&fx.store, &artifacts, &fx.project_id).unwrap();
    assert_eq!(
        item(&broken_audio, ProductionRecoveryArtifactKindV1::Audio).state,
        ProductionRecoveryArtifactStateV1::Missing
    );
    let replacement_audio = fx.temp.path().join("voice-relinked.wav");
    fs::write(&replacement_audio, wav(1_000, 7)).unwrap();
    let audio_repair = repair_creator_production_audio_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        &segment_id,
        &replacement_audio,
    )
    .unwrap();
    assert_eq!(audio_repair.timing.segment_id, segment_id);

    let after_audio =
        inspect_creator_production_recovery_v1(&fx.store, &artifacts, &fx.project_id).unwrap();
    let timing_id = item(&after_audio, ProductionRecoveryArtifactKindV1::Timing)
        .artifact_id
        .clone()
        .unwrap();
    let timing_artifact = fx.store.get_artifact(&timing_id).unwrap();
    fs::write(
        artifacts.resolve_artifact_path(&timing_artifact).unwrap(),
        b"corrupt timing",
    )
    .unwrap();
    let broken_timing =
        inspect_creator_production_recovery_v1(&fx.store, &artifacts, &fx.project_id).unwrap();
    assert_eq!(
        item(&broken_timing, ProductionRecoveryArtifactKindV1::Timing).state,
        ProductionRecoveryArtifactStateV1::Invalid
    );

    let corrected = fx.temp.path().join("timing.json");
    let timing =
        derive_manual_voice_timing_v1(&segment_id, "One recoverable production segment.", 1_000)
            .unwrap();
    fs::write(&corrected, timing.to_json_bytes_v1().unwrap()).unwrap();
    let timing_repair = repair_creator_production_timing_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        &segment_id,
        &corrected,
    )
    .unwrap();
    assert_eq!(timing_repair.timing.segment_id, segment_id);

    let rebuilt =
        rebuild_and_export_creator_production_v1(&mut fx.store, &artifacts, &fx.project_id)
            .unwrap();
    assert!(!rebuilt.export.artifacts.is_empty());

    drop(fx.store);
    let old_root = fx.workspace.data_root().to_path_buf();
    drop(fx.workspace);
    let moved_root = fx.temp.path().join("moved-data-root");
    fs::rename(&old_root, &moved_root).unwrap();
    let moved_workspace = Workspace::open(&moved_root).unwrap();
    let mut moved_store = StateStore::open(moved_workspace.sqlite_path()).unwrap();
    let moved_artifacts = ArtifactStore::new(&moved_root).unwrap();
    let moved =
        inspect_creator_production_recovery_v1(&moved_store, &moved_artifacts, &fx.project_id)
            .unwrap();
    assert!(moved.ready_for_rebuild);
    let moved_export = rebuild_and_export_creator_production_v1(
        &mut moved_store,
        &moved_artifacts,
        &fx.project_id,
    )
    .unwrap();
    assert!(!moved_export.export.cache_hit);
}
