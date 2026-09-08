use std::fs;

use omnicreator_core::artifact_store::{AttemptOutputPromotion, AttemptPromotionRequest};
use omnicreator_core::{
    choose_creator_visual_from_asset_library_v1, compile_creator_workflow_plan_v1,
    creator_scene_visual_states_v1, initial_studio_pack_catalog_v1, inspect_manual_visual_media_v1,
    materialize_creator_workflow_plan_v1, provide_manual_creator_content_v1,
    provide_manual_creator_scene_plan_v1, provide_manual_creator_visual_file_v1, ArtifactStore,
    LogicalUri, ManualResultProvenanceV1, ManualScenePlanDraftV1, ManualVisualMediaKindV1,
    StateStore, StepStatus, Workspace, CREATOR_STEP_PRODUCTION_PACK_V1,
    CREATOR_STEP_VISUAL_PREPARE_V1,
    CREATOR_STEP_VOICE_PREPARE_V1, CREATOR_WORKFLOW_UNIT_PROJECT_V1,
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
        .create_project_with_studio_pack("Manual visuals", Some(&pack.id))
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

fn prepare_content_scene(fx: &mut Fixture) {
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();
    let content = provide_manual_creator_content_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        "First visual beat.\n\nSecond visual beat.",
        ManualResultProvenanceV1::manual_editor(),
    )
    .unwrap();
    let mut draft = ManualScenePlanDraftV1::for_content_v1(&content.content).unwrap();
    for scene in &mut draft.scenes {
        scene.scene_type = "literal".to_owned();
        scene.purpose = "Support narration".to_owned();
    }
    provide_manual_creator_scene_plan_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        &draft,
        ManualResultProvenanceV1::manual_editor(),
    )
    .unwrap();
}

fn png(width: u32, height: u32, marker: u8) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes.push(marker);
    bytes
}

fn mp4() -> Vec<u8> {
    let mut bytes = 20u32.to_be_bytes().to_vec();
    bytes.extend_from_slice(b"ftyp");
    bytes.extend_from_slice(b"isom");
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(b"isom");
    bytes
}

fn project_step(store: &StateStore, project_id: &str, key: &str) -> omnicreator_core::WorkflowStep {
    store
        .list_project_steps(project_id)
        .unwrap()
        .into_iter()
        .find(|step| step.step == key && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1)
        .unwrap()
}

#[test]
fn no_stock_provider_manual_images_per_scene_unlock_visual_stage() {
    let mut fx = fixture();
    prepare_content_scene(&mut fx);
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();

    let first = fx.temp.path().join("first.png");
    let second = fx.temp.path().join("second.png");
    fs::write(&first, png(1280, 720, 1)).unwrap();
    fs::write(&second, png(1920, 1080, 2)).unwrap();

    let first_outcome = provide_manual_creator_visual_file_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        "SC001",
        &first,
        ManualResultProvenanceV1::local_file(),
        false,
    )
    .unwrap();
    assert_eq!(
        first_outcome.media.media_kind,
        ManualVisualMediaKindV1::Image
    );
    assert_eq!(first_outcome.media.width, Some(1280));
    assert!(!first_outcome.visual_stage_complete);
    assert_eq!(
        project_step(&fx.store, &fx.project_id, CREATOR_STEP_VISUAL_PREPARE_V1).status,
        StepStatus::Ready
    );

    let second_outcome = provide_manual_creator_visual_file_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        "SC002",
        &second,
        ManualResultProvenanceV1::local_file(),
        false,
    )
    .unwrap();
    assert!(second_outcome.visual_stage_complete);
    assert_eq!(
        project_step(&fx.store, &fx.project_id, CREATOR_STEP_VISUAL_PREPARE_V1).status,
        StepStatus::Succeeded
    );
    assert!(artifacts.verify_artifact(&first_outcome.artifact).unwrap());
    assert!(artifacts.verify_artifact(&second_outcome.artifact).unwrap());

    let states = creator_scene_visual_states_v1(&fx.store, &artifacts, &fx.project_id).unwrap();
    assert_eq!(states.len(), 2);
    assert!(states.iter().all(|state| state.verified));
}

#[test]
fn asset_library_visual_is_copied_into_scene_job_with_manual_provenance() {
    let mut fx = fixture();
    prepare_content_scene(&mut fx);
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();

    let source = fx.temp.path().join("library-source.png");
    fs::write(&source, png(640, 360, 3)).unwrap();
    let first = provide_manual_creator_visual_file_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        "SC001",
        &source,
        ManualResultProvenanceV1::local_file(),
        false,
    )
    .unwrap();

    let second = choose_creator_visual_from_asset_library_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        "SC002",
        &first.artifact.artifact_id,
        false,
    )
    .unwrap();

    assert_ne!(second.artifact.artifact_id, first.artifact.artifact_id);
    assert_ne!(second.artifact.uri, first.artifact.uri);
    assert_eq!(second.artifact.sha256, first.artifact.sha256);
    assert_eq!(
        second
            .artifact
            .metadata
            .get("stage")
            .and_then(|value| value.get("source_library_artifact_id"))
            .and_then(serde_json::Value::as_str),
        Some(first.artifact.artifact_id.as_str())
    );
    assert!(second.visual_stage_complete);
}

#[test]
fn replacing_one_visual_invalidates_pack_but_preserves_voice_and_other_scene() {
    let mut fx = fixture();
    prepare_content_scene(&mut fx);
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();

    let first = fx.temp.path().join("first.png");
    let second = fx.temp.path().join("second.png");
    fs::write(&first, png(1280, 720, 4)).unwrap();
    fs::write(&second, png(1280, 720, 5)).unwrap();

    let old = provide_manual_creator_visual_file_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        "SC001",
        &first,
        ManualResultProvenanceV1::local_file(),
        false,
    )
    .unwrap();
    let other = provide_manual_creator_visual_file_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        "SC002",
        &second,
        ManualResultProvenanceV1::local_file(),
        false,
    )
    .unwrap();
    assert!(other.visual_stage_complete);

    let voice = project_step(&fx.store, &fx.project_id, CREATOR_STEP_VOICE_PREPARE_V1);
    fx.store
        .set_step_status(&voice.step_id, StepStatus::Succeeded)
        .unwrap();
    fx.store.refresh_ready_steps(&fx.project_id).unwrap();
    let production = project_step(&fx.store, &fx.project_id, CREATOR_STEP_PRODUCTION_PACK_V1);
    assert_eq!(production.status, StepStatus::Ready);
    fx.store
        .set_step_status(&production.step_id, StepStatus::Succeeded)
        .unwrap();

    let replacement = fx.temp.path().join("replacement.png");
    fs::write(&replacement, png(1920, 1080, 6)).unwrap();
    let new = provide_manual_creator_visual_file_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        "SC001",
        &replacement,
        ManualResultProvenanceV1::local_file(),
        true,
    )
    .unwrap();

    assert!(new.visual_stage_complete);
    assert_eq!(
        project_step(&fx.store, &fx.project_id, CREATOR_STEP_VOICE_PREPARE_V1).status,
        StepStatus::Succeeded
    );
    assert_eq!(
        project_step(&fx.store, &fx.project_id, CREATOR_STEP_VISUAL_PREPARE_V1).status,
        StepStatus::Succeeded
    );
    assert_eq!(
        project_step(&fx.store, &fx.project_id, CREATOR_STEP_PRODUCTION_PACK_V1).status,
        StepStatus::Ready
    );

    let jobs = fx.store.list_project_jobs(&fx.project_id).unwrap();
    assert_eq!(
        jobs.iter()
            .find(|job| job.job_id == old.ingestion.job.job_id)
            .unwrap()
            .status,
        StepStatus::Stale
    );
    assert_eq!(
        jobs.iter()
            .find(|job| job.job_id == other.ingestion.job.job_id)
            .unwrap()
            .status,
        StepStatus::Succeeded
    );
}

#[test]
fn mixed_provider_image_and_manual_video_complete_visual_stage() {
    let mut fx = fixture();
    prepare_content_scene(&mut fx);
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();

    let provider_job = fx
        .store
        .create_job(
            &fx.project_id,
            CREATOR_STEP_VISUAL_PREPARE_V1,
            "SC001",
            "provider-sc001",
        )
        .unwrap();
    let provider_attempt = fx
        .store
        .start_attempt(&provider_job.job_id, Some("plugin:pexels"))
        .unwrap();
    let provider_source = fx.temp.path().join("provider.png");
    fs::write(&provider_source, png(1280, 720, 7)).unwrap();
    let provider_artifact = artifacts
        .promote_attempt_outputs(
            &mut fx.store,
            AttemptPromotionRequest {
                attempt_id: provider_attempt.attempt_id,
                job_id: provider_job.job_id.clone(),
                outputs: vec![AttemptOutputPromotion {
                    source: provider_source,
                    target_uri: LogicalUri::parse("project://visual/provider/SC001.png").unwrap(),
                    artifact_type: "image".to_owned(),
                    metadata: serde_json::json!({
                        "source_provider": "pexels",
                        "source_asset_id": "fixture-pexels-1",
                    }),
                    expected_sha256: None,
                }],
                selected_output_index: 0,
            },
        )
        .unwrap()
        .pop()
        .unwrap();

    let manual_video = fx.temp.path().join("manual.mp4");
    fs::write(&manual_video, mp4()).unwrap();
    let manual = provide_manual_creator_visual_file_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        "SC002",
        &manual_video,
        ManualResultProvenanceV1::local_file(),
        false,
    )
    .unwrap();

    assert_eq!(manual.media.media_kind, ManualVisualMediaKindV1::Video);
    assert_eq!(manual.media.mime_type, "video/mp4");
    assert!(manual.visual_stage_complete);
    assert!(artifacts.verify_artifact(&provider_artifact).unwrap());
    assert!(artifacts.verify_artifact(&manual.artifact).unwrap());
    assert_eq!(
        fx.store.get_job(&provider_job.job_id).unwrap().status,
        StepStatus::Succeeded
    );
    assert_eq!(
        project_step(&fx.store, &fx.project_id, CREATOR_STEP_VISUAL_PREPARE_V1).status,
        StepStatus::Succeeded
    );
}

#[test]
fn media_inspection_rejects_extension_spoofing() {
    let fx = fixture();
    let fake = fx.temp.path().join("fake.png");
    fs::write(&fake, b"not a png").unwrap();
    assert!(inspect_manual_visual_media_v1(&fake).is_err());

    let unsupported = fx.temp.path().join("asset.bmp");
    fs::write(&unsupported, b"BM").unwrap();
    assert!(inspect_manual_visual_media_v1(&unsupported).is_err());
}
