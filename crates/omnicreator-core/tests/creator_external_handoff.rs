use std::fs;

use omnicreator_core::{
    compile_creator_workflow_plan_v1, creator_scene_visual_states_v1,
    creator_segment_voice_states_v1, derive_manual_voice_timing_v1, initial_studio_pack_catalog_v1,
    materialize_creator_workflow_plan_v1, prepare_external_generated_visual_request_v1,
    prepare_external_voice_request_v1, provide_external_generated_visual_result_v1,
    provide_external_voice_result_v1, provide_manual_creator_content_v1,
    provide_manual_creator_scene_plan_v1, provide_manual_creator_visual_file_v1, ArtifactStore,
    ManualResultProvenanceV1, ManualScenePlanDraftV1, StateStore, StepStatus, Workspace,
    CREATOR_STEP_PRODUCTION_PACK_V1, CREATOR_STEP_VISUAL_PREPARE_V1, CREATOR_STEP_VOICE_PREPARE_V1,
    CREATOR_TTS_STEP_V1, CREATOR_WORKFLOW_UNIT_PROJECT_V1,
};

struct Fixture {
    temp: tempfile::TempDir,
    workspace: Workspace,
    store: StateStore,
    project_id: String,
}

fn fixture(title: &str) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let store = StateStore::open(workspace.sqlite_path()).unwrap();
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let project = store
        .create_project_with_studio_pack(title, Some(&pack.id))
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

fn prepare_content_scene(fx: &mut Fixture, script: &str) {
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();
    let content = provide_manual_creator_content_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        script,
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

fn wav(duration_ms: u64, marker: u8) -> Vec<u8> {
    let sample_rate = 8_000u32;
    let channels = 1u16;
    let bits = 16u16;
    let block_align = channels * (bits / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let data_size = ((u64::from(byte_rate) * duration_ms) / 1_000) as u32;
    let riff_size = 36u32 + data_size;
    let mut out = Vec::with_capacity((44 + data_size) as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
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

fn project_step(store: &StateStore, project_id: &str, key: &str) -> omnicreator_core::WorkflowStep {
    store
        .list_project_steps(project_id)
        .unwrap()
        .into_iter()
        .find(|step| step.step == key && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1)
        .unwrap()
}

#[test]
fn provider_unavailable_external_generated_result_succeeds_with_truthful_provenance() {
    let mut fx = fixture("External visual");
    prepare_content_scene(&mut fx, "One generated visual.");
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();
    let request = prepare_external_generated_visual_request_v1(
        &fx.store,
        &artifacts,
        &fx.project_id,
        "SC001",
    )
    .unwrap();

    let serialized = request.to_pretty_json_v1().unwrap();
    assert!(!serialized.contains(fx.temp.path().to_string_lossy().as_ref()));
    assert!(!serialized.contains("provider_id"));
    assert!(!serialized.contains("plugin_id"));
    assert!(!serialized.to_ascii_lowercase().contains("api_key"));
    assert!(!serialized.to_ascii_lowercase().contains("authorization"));

    let result_path = fx.temp.path().join("external.png");
    fs::write(&result_path, png(1280, 720, 9)).unwrap();
    let outcome = provide_external_generated_visual_result_v1(
        &mut fx.store,
        &artifacts,
        &request,
        &result_path,
        "external-tool",
        false,
    )
    .unwrap();

    assert_eq!(
        outcome.ingestion.attempt.worker.as_deref(),
        Some("manual-result:external")
    );
    assert_eq!(
        outcome
            .artifact
            .metadata
            .get("manual_result")
            .and_then(|value| value.get("provenance"))
            .and_then(|value| value.get("producer"))
            .and_then(serde_json::Value::as_str),
        Some("EXTERNAL_RESULT_IMPORT")
    );
    assert_eq!(
        outcome
            .artifact
            .metadata
            .get("manual_result")
            .and_then(|value| value.get("provenance"))
            .and_then(|value| value.get("request_sha256"))
            .and_then(serde_json::Value::as_str),
        Some(request.request_sha256.as_str())
    );
    assert!(artifacts.verify_artifact(&outcome.artifact).unwrap());
    assert_eq!(
        project_step(&fx.store, &fx.project_id, CREATOR_STEP_VISUAL_PREPARE_V1).status,
        StepStatus::Succeeded
    );
    assert!(fx
        .store
        .list_attempts(&outcome.ingestion.job.job_id)
        .unwrap()
        .iter()
        .all(|attempt| attempt.worker.as_deref() != Some("compute-provider")));
}

#[test]
fn external_visual_replacement_invalidates_only_the_downstream_cone() {
    let mut fx = fixture("External replacement");
    prepare_content_scene(&mut fx, "First scene.\n\nSecond scene.");
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();

    let request = prepare_external_generated_visual_request_v1(
        &fx.store,
        &artifacts,
        &fx.project_id,
        "SC001",
    )
    .unwrap();
    let external = fx.temp.path().join("external-first.png");
    fs::write(&external, png(1280, 720, 1)).unwrap();
    let first = provide_external_generated_visual_result_v1(
        &mut fx.store,
        &artifacts,
        &request,
        &external,
        "external-tool",
        false,
    )
    .unwrap();

    let manual = fx.temp.path().join("manual-second.png");
    fs::write(&manual, png(1280, 720, 2)).unwrap();
    let second = provide_manual_creator_visual_file_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        "SC002",
        &manual,
        ManualResultProvenanceV1::local_file(),
        false,
    )
    .unwrap();
    assert!(second.visual_stage_complete);

    let voice = project_step(&fx.store, &fx.project_id, CREATOR_STEP_VOICE_PREPARE_V1);
    fx.store
        .set_step_status(&voice.step_id, StepStatus::Succeeded)
        .unwrap();
    fx.store.refresh_ready_steps(&fx.project_id).unwrap();
    let production = project_step(&fx.store, &fx.project_id, CREATOR_STEP_PRODUCTION_PACK_V1);
    fx.store
        .set_step_status(&production.step_id, StepStatus::Succeeded)
        .unwrap();

    let replacement = fx.temp.path().join("external-replacement.png");
    fs::write(&replacement, png(1920, 1080, 3)).unwrap();
    let replaced = provide_external_generated_visual_result_v1(
        &mut fx.store,
        &artifacts,
        &request,
        &replacement,
        "external-tool",
        true,
    )
    .unwrap();

    assert!(replaced.visual_stage_complete);
    assert_eq!(
        project_step(&fx.store, &fx.project_id, CREATOR_STEP_VOICE_PREPARE_V1).status,
        StepStatus::Succeeded
    );
    assert_eq!(
        project_step(&fx.store, &fx.project_id, CREATOR_STEP_PRODUCTION_PACK_V1).status,
        StepStatus::Ready
    );
    let jobs = fx.store.list_project_jobs(&fx.project_id).unwrap();
    assert_eq!(
        jobs.iter()
            .find(|job| job.job_id == first.ingestion.job.job_id)
            .unwrap()
            .status,
        StepStatus::Stale
    );
    assert_eq!(
        jobs.iter()
            .find(|job| job.job_id == second.ingestion.job.job_id)
            .unwrap()
            .status,
        StepStatus::Succeeded
    );
}

#[test]
fn external_voice_audio_and_timing_rejoin_canonical_voice_take() {
    let mut fx = fixture("External voice");
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();
    provide_manual_creator_content_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        "Narrate this externally.",
        ManualResultProvenanceV1::manual_editor(),
    )
    .unwrap();

    let request =
        prepare_external_voice_request_v1(&fx.store, &artifacts, &fx.project_id, "SEG001").unwrap();
    let serialized = request.to_pretty_json_v1().unwrap();
    assert!(!serialized.contains("provider_id"));
    assert!(!serialized.contains("plugin_id"));
    assert!(!serialized.contains(fx.temp.path().to_string_lossy().as_ref()));

    let audio = fx.temp.path().join("external.wav");
    fs::write(&audio, wav(1_000, 7)).unwrap();
    let timing =
        derive_manual_voice_timing_v1("SEG001", "Narrate this externally.", 1_000).unwrap();
    let outcome = provide_external_voice_result_v1(
        &mut fx.store,
        &artifacts,
        &request,
        &audio,
        timing,
        "external-tool",
        false,
    )
    .unwrap();

    assert!(outcome.voice_stage_complete);
    assert!(artifacts.verify_artifact(&outcome.audio).unwrap());
    assert!(artifacts.verify_artifact(&outcome.timing_artifact).unwrap());
    let attempt = fx.store.get_attempt(&outcome.attempt_id).unwrap();
    assert_eq!(attempt.worker.as_deref(), Some("manual-result:external"));
    assert_eq!(outcome.job.step, CREATOR_TTS_STEP_V1);
    assert_eq!(outcome.job.unit, "SEG001");
    assert_eq!(outcome.job.status, StepStatus::Succeeded);
    let states = creator_segment_voice_states_v1(&fx.store, &artifacts, &fx.project_id).unwrap();
    assert_eq!(states.len(), 1);
    assert!(states[0].verified);
}

#[test]
fn external_result_survives_restart_and_data_root_move_without_shadow_state() {
    let mut fx = fixture("Portable external");
    prepare_content_scene(&mut fx, "Portable external scene.");
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();
    let request = prepare_external_generated_visual_request_v1(
        &fx.store,
        &artifacts,
        &fx.project_id,
        "SC001",
    )
    .unwrap();
    let image = fx.temp.path().join("portable.png");
    fs::write(&image, png(1280, 720, 4)).unwrap();
    let outcome = provide_external_generated_visual_result_v1(
        &mut fx.store,
        &artifacts,
        &request,
        &image,
        "external-tool",
        false,
    )
    .unwrap();
    let artifact_id = outcome.artifact.artifact_id.clone();
    let project_id = fx.project_id.clone();
    let root = fx.workspace.data_root().to_path_buf();

    drop(artifacts);
    drop(fx.store);
    drop(fx.workspace);

    let moved = fx.temp.path().join("moved-data");
    fs::rename(&root, &moved).unwrap();
    let workspace = Workspace::open(&moved).unwrap();
    let store = StateStore::open(workspace.sqlite_path()).unwrap();
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let artifact = store.get_artifact(&artifact_id).unwrap();

    assert!(artifacts.verify_artifact(&artifact).unwrap());
    assert!(!artifact
        .uri
        .as_str()
        .contains(moved.to_string_lossy().as_ref()));
    assert_eq!(
        project_step(&store, &project_id, CREATOR_STEP_VISUAL_PREPARE_V1).status,
        StepStatus::Succeeded
    );
    let states = creator_scene_visual_states_v1(&store, &artifacts, &project_id).unwrap();
    assert_eq!(states.len(), 1);
    assert!(states[0].verified);

    let jobs = store.list_project_jobs(&project_id).unwrap();
    assert_eq!(
        jobs.iter()
            .filter(|job| job.step == CREATOR_STEP_VISUAL_PREPARE_V1 && job.unit == "SC001")
            .count(),
        1
    );
    assert_eq!(
        store
            .list_attempts(
                &jobs
                    .iter()
                    .find(|job| job.step == CREATOR_STEP_VISUAL_PREPARE_V1 && job.unit == "SC001")
                    .unwrap()
                    .job_id
            )
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn mixed_automatic_manual_and_external_visual_producers_share_one_canonical_stage() {
    let mut fx = fixture("Mixed producers");
    prepare_content_scene(
        &mut fx,
        "Provider scene.\n\nManual scene.\n\nExternal scene.",
    );
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();

    let provider_file = fx.temp.path().join("provider.png");
    fs::write(&provider_file, png(1280, 720, 10)).unwrap();
    let provider_job = fx
        .store
        .create_job(
            &fx.project_id,
            CREATOR_STEP_VISUAL_PREPARE_V1,
            "SC001",
            "provider-input",
        )
        .unwrap();
    let provider_attempt = fx
        .store
        .start_attempt(&provider_job.job_id, Some("plugin:fixture"))
        .unwrap();
    let provider_artifact = artifacts
        .promote_attempt_outputs(
            &mut fx.store,
            omnicreator_core::artifact_store::AttemptPromotionRequest {
                attempt_id: provider_attempt.attempt_id,
                job_id: provider_job.job_id,
                outputs: vec![omnicreator_core::artifact_store::AttemptOutputPromotion {
                    source: provider_file,
                    target_uri: omnicreator_core::LogicalUri::parse(
                        "project://visual/provider/SC001.png",
                    )
                    .unwrap(),
                    artifact_type: "image".to_owned(),
                    metadata: serde_json::json!({"source_provider": "fixture"}),
                    expected_sha256: None,
                }],
                selected_output_index: 0,
            },
        )
        .unwrap()
        .pop()
        .unwrap();

    let manual_file = fx.temp.path().join("manual.png");
    fs::write(&manual_file, png(1280, 720, 11)).unwrap();
    let manual = provide_manual_creator_visual_file_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        "SC002",
        &manual_file,
        ManualResultProvenanceV1::local_file(),
        false,
    )
    .unwrap();

    let request = prepare_external_generated_visual_request_v1(
        &fx.store,
        &artifacts,
        &fx.project_id,
        "SC003",
    )
    .unwrap();
    let external_file = fx.temp.path().join("external.png");
    fs::write(&external_file, png(1280, 720, 12)).unwrap();
    let external = provide_external_generated_visual_result_v1(
        &mut fx.store,
        &artifacts,
        &request,
        &external_file,
        "external-tool",
        false,
    )
    .unwrap();

    assert!(artifacts.verify_artifact(&provider_artifact).unwrap());
    assert!(artifacts.verify_artifact(&manual.artifact).unwrap());
    assert!(artifacts.verify_artifact(&external.artifact).unwrap());
    assert!(external.visual_stage_complete);
    assert_eq!(
        project_step(&fx.store, &fx.project_id, CREATOR_STEP_VISUAL_PREPARE_V1).status,
        StepStatus::Succeeded
    );
    assert_eq!(
        external
            .artifact
            .metadata
            .get("manual_result")
            .and_then(|value| value.get("provenance"))
            .and_then(|value| value.get("producer"))
            .and_then(serde_json::Value::as_str),
        Some("EXTERNAL_RESULT_IMPORT")
    );
}
