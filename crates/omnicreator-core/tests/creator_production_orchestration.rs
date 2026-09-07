use std::fs;

use chrono::Utc;
use omnicreator_core::artifact_store::{AttemptOutputPromotion, AttemptPromotionRequest};
use omnicreator_core::{
    assemble_creator_production_pack_v1, compile_creator_workflow_plan_v1,
    initial_studio_pack_catalog_v1, load_latest_creator_production_pack_v1,
    materialize_creator_workflow_plan_v1, Artifact, ArtifactStore, CreatorContentV1,
    CreatorInputV1, CreatorProductionPackOptionsV1, CreatorScenePlanV1, LogicalUri, PathResolver,
    ProductionPackageExporterV1, SceneIntentV1, SegmentV1, StateStore, StepStatus,
    VoiceDirectionV1, VoiceTimingCueV1, VoiceTimingV1, Workspace, CREATOR_CONTENT_ARTIFACT_TYPE_V1,
    CREATOR_CONTENT_SCHEMA_V1, CREATOR_CONTENT_VERSION_V1, CREATOR_SCENE_PLAN_ARTIFACT_TYPE_V1,
    CREATOR_SCENE_PLAN_SCHEMA_V1, CREATOR_SCENE_PLAN_VERSION_V1, CREATOR_STEP_CONTENT_PREPARE_V1,
    CREATOR_STEP_PRODUCTION_PACK_V1, CREATOR_STEP_SCENE_PLAN_V1, CREATOR_STEP_VISUAL_PREPARE_V1,
    CREATOR_STEP_VOICE_PREPARE_V1, CREATOR_TTS_STEP_V1, CREATOR_WORKFLOW_UNIT_PROJECT_V1,
    SCENE_INTENT_SCHEMA, SCENE_INTENT_SCHEMA_VERSION, SEGMENT_SCHEMA, SEGMENT_SCHEMA_VERSION,
    VOICE_AUDIO_ARTIFACT_TYPE_V1, VOICE_TIMING_ARTIFACT_TYPE_V1, VOICE_TIMING_SCHEMA_V1,
};
use sha2::{Digest, Sha256};

struct Fixture {
    _temp: tempfile::TempDir,
    workspace: Workspace,
    store: StateStore,
    project_id: String,
    content: CreatorContentV1,
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
        .create_project_with_studio_pack("P4 Production", Some(&pack.id))
        .unwrap();
    let workflow = compile_creator_workflow_plan_v1(&project, &pack).unwrap();
    materialize_creator_workflow_plan_v1(&store, &workflow).unwrap();

    let content = CreatorContentV1 {
        schema: CREATOR_CONTENT_SCHEMA_V1.to_owned(),
        schema_version: CREATOR_CONTENT_VERSION_V1,
        project_id: project.id.clone(),
        source: CreatorInputV1::script("First line.\n\nSecond line."),
        script: "First line.\n\nSecond line.".to_owned(),
        segments: vec![
            segment("S001", 1, "First line."),
            segment("S002", 2, "Second line."),
        ],
    };
    content.validate_v1().unwrap();

    Fixture {
        _temp: temp,
        workspace,
        store,
        project_id: project.id,
        content,
    }
}

fn segment(id: &str, order: u32, text: &str) -> SegmentV1 {
    SegmentV1 {
        schema: SEGMENT_SCHEMA.to_owned(),
        schema_version: SEGMENT_SCHEMA_VERSION,
        id: id.to_owned(),
        order,
        text: text.to_owned(),
        voice_direction: VoiceDirectionV1::default(),
    }
}

fn scene(id: &str, segment: &SegmentV1) -> SceneIntentV1 {
    SceneIntentV1 {
        schema: SCENE_INTENT_SCHEMA.to_owned(),
        schema_version: SCENE_INTENT_SCHEMA_VERSION,
        id: id.to_owned(),
        segment_id: segment.id.clone(),
        narration: segment.text.clone(),
        purpose: format!("Purpose for {id}"),
        scene_type: "literal".to_owned(),
        emotion_before: None,
        emotion_after: None,
        duration_hint: None,
        visual_ideas: vec!["A concrete visual".to_owned()],
        search_queries: vec!["concrete visual".to_owned()],
        avoid: vec![],
        continuity: Default::default(),
        aspect_ratio: "16:9".to_owned(),
    }
}

fn step_id(store: &StateStore, project_id: &str, key: &str) -> String {
    store
        .list_project_steps(project_id)
        .unwrap()
        .into_iter()
        .find(|step| step.step == key && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1)
        .unwrap()
        .step_id
}

fn promote_json<T: serde::Serialize>(
    fixture: &mut Fixture,
    step: &str,
    unit: &str,
    input_hash: &str,
    value: &T,
    artifact_type: &str,
    uri: &str,
) -> Artifact {
    let job = fixture
        .store
        .create_job(&fixture.project_id, step, unit, input_hash)
        .unwrap();
    let attempt = fixture
        .store
        .start_attempt(&job.job_id, Some("p4-fixture"))
        .unwrap();
    let source = fixture
        ._temp
        .path()
        .join(format!("{}-{}.json", step.replace('.', "-"), unit));
    fs::write(&source, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    ArtifactStore::new(fixture.workspace.data_root())
        .unwrap()
        .promote_attempt_outputs(
            &mut fixture.store,
            AttemptPromotionRequest {
                attempt_id: attempt.attempt_id,
                job_id: job.job_id,
                outputs: vec![AttemptOutputPromotion {
                    source,
                    target_uri: LogicalUri::parse(uri).unwrap(),
                    artifact_type: artifact_type.to_owned(),
                    metadata: serde_json::json!({"fixture": "phase15-p4"}),
                    expected_sha256: None,
                }],
                selected_output_index: 0,
            },
        )
        .unwrap()
        .pop()
        .unwrap()
}

fn promote_bytes(fixture: &mut Fixture, scene_id: &str, bytes: &[u8]) -> Artifact {
    let input_hash = format!("visual-input-{scene_id}");
    let job = fixture
        .store
        .create_job(
            &fixture.project_id,
            CREATOR_STEP_VISUAL_PREPARE_V1,
            scene_id,
            &input_hash,
        )
        .unwrap();
    let attempt = fixture
        .store
        .start_attempt(&job.job_id, Some("visual-fixture"))
        .unwrap();
    let source = fixture._temp.path().join(format!("{scene_id}.png"));
    fs::write(&source, bytes).unwrap();
    ArtifactStore::new(fixture.workspace.data_root())
        .unwrap()
        .promote_attempt_outputs(
            &mut fixture.store,
            AttemptPromotionRequest {
                attempt_id: attempt.attempt_id,
                job_id: job.job_id,
                outputs: vec![AttemptOutputPromotion {
                    source,
                    target_uri: LogicalUri::parse(&format!("project://visual/{scene_id}.png"))
                        .unwrap(),
                    artifact_type: "image".to_owned(),
                    metadata: serde_json::json!({"visual_routing": {"fixture": true}}),
                    expected_sha256: None,
                }],
                selected_output_index: 0,
            },
        )
        .unwrap()
        .pop()
        .unwrap()
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn commit_voice(
    fixture: &mut Fixture,
    segment_id: &str,
    duration_ms: u64,
    cue_text: &str,
) -> (Artifact, Artifact) {
    let job = fixture
        .store
        .create_job(
            &fixture.project_id,
            CREATOR_TTS_STEP_V1,
            segment_id,
            &format!("voice-input-{segment_id}"),
        )
        .unwrap();
    let started = fixture
        .store
        .start_voice_take_attempt_v1(&job.job_id, Some("voice-fixture"))
        .unwrap();

    let audio_uri =
        LogicalUri::parse(&format!("project://audio/{segment_id}/take-0001.wav")).unwrap();
    let timing_uri = LogicalUri::parse(&format!(
        "project://audio/{segment_id}/take-0001.timing.json"
    ))
    .unwrap();
    let resolver = PathResolver::new(fixture.workspace.data_root()).unwrap();
    let audio_path = resolver
        .resolve(&audio_uri, Some(&fixture.project_id))
        .unwrap();
    let timing_path = resolver
        .resolve(&timing_uri, Some(&fixture.project_id))
        .unwrap();
    fs::create_dir_all(audio_path.parent().unwrap()).unwrap();

    let audio_bytes = format!("fake audio {segment_id}").into_bytes();
    fs::write(&audio_path, &audio_bytes).unwrap();
    let timing_contract = VoiceTimingV1 {
        schema: VOICE_TIMING_SCHEMA_V1.to_owned(),
        version: 1,
        segment_id: segment_id.to_owned(),
        duration_ms,
        cues: vec![VoiceTimingCueV1 {
            index: 0,
            text: cue_text.to_owned(),
            start_ms: 0,
            end_ms: duration_ms,
        }],
    };
    let timing_bytes = timing_contract.to_json_bytes_v1().unwrap();
    fs::write(&timing_path, &timing_bytes).unwrap();

    let audio = Artifact {
        artifact_id: format!("audio-{segment_id}"),
        project_id: Some(fixture.project_id.clone()),
        artifact_type: VOICE_AUDIO_ARTIFACT_TYPE_V1.to_owned(),
        uri: audio_uri,
        sha256: sha256(&audio_bytes),
        size_bytes: audio_bytes.len() as u64,
        input_hash: Some(job.input_hash.clone()),
        producer_job: Some(job.job_id.clone()),
        created_at: Utc::now(),
        metadata: serde_json::json!({"fixture": "voice"}),
    };
    let timing = Artifact {
        artifact_id: format!("timing-{segment_id}"),
        project_id: Some(fixture.project_id.clone()),
        artifact_type: VOICE_TIMING_ARTIFACT_TYPE_V1.to_owned(),
        uri: timing_uri,
        sha256: sha256(&timing_bytes),
        size_bytes: timing_bytes.len() as u64,
        input_hash: Some(job.input_hash),
        producer_job: Some(job.job_id),
        created_at: Utc::now(),
        metadata: serde_json::json!({"fixture": "timing"}),
    };

    fixture
        .store
        .commit_remote_voice_bundle_success_v1(&started.attempt.attempt_id, &audio, &timing)
        .unwrap();
    (audio, timing)
}

fn prepare_canonical_outputs(fixture: &mut Fixture) {
    let artifacts = ArtifactStore::new(fixture.workspace.data_root()).unwrap();

    let content_hash = "content-stage-hash";
    let content_value = fixture.content.clone();
    let content_artifact = promote_json(
        fixture,
        CREATOR_STEP_CONTENT_PREPARE_V1,
        CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        content_hash,
        &content_value,
        CREATOR_CONTENT_ARTIFACT_TYPE_V1,
        "project://content/content.json",
    );
    assert!(artifacts.verify_artifact(&content_artifact).unwrap());
    let content_step = step_id(
        &fixture.store,
        &fixture.project_id,
        CREATOR_STEP_CONTENT_PREPARE_V1,
    );
    fixture
        .store
        .set_step_status(&content_step, StepStatus::Succeeded)
        .unwrap();
    fixture
        .store
        .refresh_ready_steps(&fixture.project_id)
        .unwrap();

    let scene_plan = CreatorScenePlanV1 {
        schema: CREATOR_SCENE_PLAN_SCHEMA_V1.to_owned(),
        schema_version: CREATOR_SCENE_PLAN_VERSION_V1,
        project_id: fixture.project_id.clone(),
        content_sha256: content_artifact.sha256.clone(),
        scenes: vec![
            scene("SC001", &fixture.content.segments[0]),
            scene("SC002", &fixture.content.segments[1]),
        ],
    };
    scene_plan.validate_v1(&fixture.content).unwrap();
    let scene_artifact = promote_json(
        fixture,
        CREATOR_STEP_SCENE_PLAN_V1,
        CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        "scene-stage-hash",
        &scene_plan,
        CREATOR_SCENE_PLAN_ARTIFACT_TYPE_V1,
        "project://scenes/scene-plan.json",
    );
    assert!(artifacts.verify_artifact(&scene_artifact).unwrap());
    let scene_step = step_id(
        &fixture.store,
        &fixture.project_id,
        CREATOR_STEP_SCENE_PLAN_V1,
    );
    fixture
        .store
        .set_step_status(&scene_step, StepStatus::Succeeded)
        .unwrap();
    fixture
        .store
        .refresh_ready_steps(&fixture.project_id)
        .unwrap();

    promote_bytes(fixture, "SC001", b"visual one");
    promote_bytes(fixture, "SC002", b"visual two");
    let visual_step = step_id(
        &fixture.store,
        &fixture.project_id,
        CREATOR_STEP_VISUAL_PREPARE_V1,
    );
    fixture
        .store
        .set_step_status(&visual_step, StepStatus::Succeeded)
        .unwrap();

    commit_voice(fixture, "S001", 1_000, "First line.");
    commit_voice(fixture, "S002", 1_500, "Second line.");
    let voice_step = step_id(
        &fixture.store,
        &fixture.project_id,
        CREATOR_STEP_VOICE_PREPARE_V1,
    );
    fixture
        .store
        .set_step_status(&voice_step, StepStatus::Succeeded)
        .unwrap();
    fixture
        .store
        .refresh_ready_steps(&fixture.project_id)
        .unwrap();

    assert_eq!(
        fixture
            .store
            .get_step(&step_id(
                &fixture.store,
                &fixture.project_id,
                CREATOR_STEP_PRODUCTION_PACK_V1
            ))
            .unwrap()
            .status,
        StepStatus::Ready
    );
}

#[test]
fn p4_assembles_stable_timeline_from_selected_visuals_and_voice_timing() {
    let mut fixture = fixture();
    prepare_canonical_outputs(&mut fixture);
    let artifacts = ArtifactStore::new(fixture.workspace.data_root()).unwrap();

    let outcome = assemble_creator_production_pack_v1(
        &mut fixture.store,
        &artifacts,
        &fixture.project_id,
        &CreatorProductionPackOptionsV1::default(),
    )
    .unwrap();

    assert!(!outcome.cache_hit);
    assert_eq!(outcome.total_duration_ms, 2_500);
    assert_eq!(outcome.production_pack.tracks.len(), 2);
    assert_eq!(
        outcome.production_pack.tracks[0].role,
        omnicreator_core::TimelineTrackRoleV1::VideoPrimary
    );
    assert_eq!(
        outcome.production_pack.tracks[0].clips[0].timeline_start_ms,
        0
    );
    assert_eq!(
        outcome.production_pack.tracks[0].clips[1].timeline_start_ms,
        1_000
    );
    assert_eq!(
        outcome.production_pack.tracks[1].clips[1].duration_ms,
        1_500
    );
    assert_eq!(outcome.production_pack.subtitles[0].start_ms, 0);
    assert_eq!(outcome.production_pack.subtitles[0].end_ms, 1_000);
    assert_eq!(outcome.production_pack.subtitles[1].start_ms, 1_000);
    assert_eq!(outcome.production_pack.subtitles[1].end_ms, 2_500);
    assert_eq!(outcome.production_pack.markers[1].position_ms, 1_000);
    assert!(outcome
        .production_pack
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .all(|clip| clip.uri.as_str().starts_with("project://")));
    assert!(artifacts.verify_artifact(&outcome.artifact).unwrap());

    let second = assemble_creator_production_pack_v1(
        &mut fixture.store,
        &artifacts,
        &fixture.project_id,
        &CreatorProductionPackOptionsV1::default(),
    )
    .unwrap();
    assert!(second.cache_hit);
    assert_eq!(second.artifact.artifact_id, outcome.artifact.artifact_id);
    assert_eq!(second.production_pack, outcome.production_pack);
}

#[test]
fn p4_assembled_pack_exports_through_existing_phase9_package_exporter() {
    let mut fixture = fixture();
    prepare_canonical_outputs(&mut fixture);
    let artifacts = ArtifactStore::new(fixture.workspace.data_root()).unwrap();
    let assembled = assemble_creator_production_pack_v1(
        &mut fixture.store,
        &artifacts,
        &fixture.project_id,
        &CreatorProductionPackOptionsV1::default(),
    )
    .unwrap();

    let exported = ProductionPackageExporterV1::default()
        .export_v1(&mut fixture.store, &artifacts, &assembled.production_pack)
        .unwrap();

    assert!(!exported.cache_hit);
    assert_eq!(exported.artifacts.len(), 4);
    assert!(exported
        .artifacts
        .iter()
        .all(|artifact| artifacts.verify_artifact(artifact).unwrap()));
    assert!(exported
        .artifacts
        .iter()
        .any(|artifact| artifact.uri.as_str().ends_with("edit.fcpxml")));
    assert!(exported
        .artifacts
        .iter()
        .any(|artifact| artifact.uri.as_str().ends_with("subtitles.srt")));
}

#[test]
fn p4_latest_assembly_survives_data_root_move_in_read_only_mode() {
    let mut fixture = fixture();
    prepare_canonical_outputs(&mut fixture);
    let artifacts = ArtifactStore::new(fixture.workspace.data_root()).unwrap();
    let assembled = assemble_creator_production_pack_v1(
        &mut fixture.store,
        &artifacts,
        &fixture.project_id,
        &CreatorProductionPackOptionsV1::default(),
    )
    .unwrap();

    let parent = fixture._temp.path().to_path_buf();
    let source = fixture.workspace.data_root().to_path_buf();
    let moved = parent.join("moved-data");
    drop(fixture.store);
    drop(fixture.workspace);
    fs::rename(&source, &moved).unwrap();

    let reopened = Workspace::open(&moved).unwrap();
    let read_only = StateStore::open_read_only(reopened.sqlite_path()).unwrap();
    let moved_artifacts = ArtifactStore::new(reopened.data_root()).unwrap();
    let loaded =
        load_latest_creator_production_pack_v1(&read_only, &moved_artifacts, &fixture.project_id)
            .unwrap()
            .unwrap();

    assert_eq!(loaded.production_pack, assembled.production_pack);
    assert_eq!(loaded.input_hash, assembled.input_hash);
    assert_eq!(loaded.total_duration_ms, 2_500);
    let json = serde_json::to_string(&loaded.production_pack).unwrap();
    assert!(!json.contains(source.to_string_lossy().as_ref()));
    assert!(!json.contains(moved.to_string_lossy().as_ref()));
    assert!(read_only
        .update_project_title(&fixture.project_id, "Forbidden")
        .is_err());
}
