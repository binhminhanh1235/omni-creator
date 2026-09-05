use std::{collections::BTreeMap, path::PathBuf};

use chrono::{TimeZone, Utc};
use omnicreator_core::{
    evaluate_gpu_queue, scan_plugin_roots, ArtifactStore, CacheLookupV1,
    ComputeDeviceV1, ComputeProviderCapabilitiesV1, ComputeProviderConnectionState,
    ComputeProviderSchedulingSnapshotV1, ComputeProviderSessionIdentityV1,
    ComputeProviderSessionV1, GeneratedImagePreparationV1, GeneratedImageRequestV1,
    GeneratedImageResolutionV1, GeneratedImageStyleV1, GpuQueueEligibilityStatusV1,
    GpuReadinessFactsV1, LogicalUri, PluginProcessOptions, SceneIntentV1, StateStore,
    StepStatus, Workspace, SCENE_INTENT_SCHEMA, SCENE_INTENT_SCHEMA_VERSION,
};

fn plugin_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}

fn generated_plugin() -> omnicreator_core::DiscoveredPlugin {
    let report = scan_plugin_roots(&[plugin_root()]);
    assert!(
        report.diagnostics.is_empty(),
        "plugin discovery diagnostics: {:?}",
        report.diagnostics
    );
    report
        .registry
        .get("generated-image-reference")
        .expect("generated image reference plugin")
        .clone()
}

fn scene() -> SceneIntentV1 {
    SceneIntentV1 {
        schema: SCENE_INTENT_SCHEMA.to_owned(),
        schema_version: SCENE_INTENT_SCHEMA_VERSION,
        id: "SC01".to_owned(),
        segment_id: "S01".to_owned(),
        narration: "A quiet craftsperson repairs a weathered wooden gate.".to_owned(),
        purpose: "Show patient restoration without a literal talking head.".to_owned(),
        scene_type: "conceptual".to_owned(),
        emotion_before: Some("worn".to_owned()),
        emotion_after: Some("hopeful".to_owned()),
        duration_hint: Some(5.0),
        visual_ideas: vec![
            "warm dawn light".to_owned(),
            "close hands repairing wood".to_owned(),
        ],
        search_queries: vec!["repairing wooden gate".to_owned()],
        avoid: vec!["logos".to_owned(), "text overlays".to_owned()],
        continuity: BTreeMap::new(),
        aspect_ratio: "16:9".to_owned(),
    }
}

fn request() -> GeneratedImageRequestV1 {
    GeneratedImageRequestV1::from_scene_v1(
        scene(),
        GeneratedImageStyleV1 {
            preset: "cinematic-warm".to_owned(),
            description: Some("natural texture, restrained contrast".to_owned()),
        },
        GeneratedImageResolutionV1 {
            width: 1280,
            height: 720,
        },
        Some(42),
        BTreeMap::new(),
    )
    .unwrap()
}

fn preparation(
    provider_id: Option<&str>,
    gpu_execution_requested: bool,
) -> GeneratedImagePreparationV1 {
    GeneratedImagePreparationV1 {
        request: request(),
        output_uri: Some(LogicalUri::parse("project://visual/SC01.svg").unwrap()),
        provider_id: provider_id.map(ToOwned::to_owned),
        model_id: Some("reference-svg".to_owned()),
        model_version: Some("1".to_owned()),
        approval_required: false,
        approval_complete: true,
        production_lock_required: false,
        gpu_execution_requested,
    }
}

#[test]
fn scene_intent_executes_through_visual_generate_and_core_commits_verified_artifact() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Generated Image P0").unwrap();

    let plugin = generated_plugin();
    let preparation = preparation(None, false);
    let input_hash = preparation.request.input_hash_v1().unwrap();
    let job = state
        .create_job(
            &project.id,
            "visual.generate",
            &preparation.request.scene.id,
            &input_hash,
        )
        .unwrap();

    let artifact_store = ArtifactStore::new(workspace.data_root()).unwrap();
    let execution = omnicreator_core::execute_generated_image_plugin_v1(
        &mut state,
        &artifact_store,
        &plugin,
        temp.path().join("plugin-runtime"),
        &job.job_id,
        &preparation,
        PluginProcessOptions::default(),
    )
    .unwrap();

    let persisted_job = state.get_job(&job.job_id).unwrap();
    assert_eq!(persisted_job.status, StepStatus::Succeeded);
    assert_eq!(
        persisted_job.selected_attempt.as_deref(),
        Some(execution.attempt_id.as_str())
    );
    assert_eq!(
        persisted_job.selected_artifact.as_deref(),
        Some(execution.artifact.artifact_id.as_str())
    );

    let attempts = state.list_attempts(&job.job_id).unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].status, StepStatus::Succeeded);
    assert!(artifact_store.verify_artifact(&execution.artifact).unwrap());

    assert_eq!(execution.artifact.metadata["source"], "generated");
    assert_eq!(
        execution.artifact.metadata["provider"],
        "generated-image-reference"
    );
    assert_eq!(execution.artifact.metadata["model"]["id"], "reference-svg");
    assert_eq!(execution.artifact.metadata["model"]["version"], "1");
    assert_eq!(execution.artifact.metadata["seed"], 42);
    assert_eq!(
        execution.artifact.metadata["settings_fingerprint"],
        preparation.request.settings_fingerprint
    );
    assert_eq!(
        execution.artifact.metadata["prompt_sha256"],
        preparation.request.prompt_sha256
    );

    let metadata = serde_json::to_string(&execution.artifact.metadata).unwrap();
    assert!(!metadata.contains(temp.path().to_string_lossy().as_ref()));
    assert!(!metadata.to_ascii_lowercase().contains("api_key"));
    assert!(!execution
        .artifact
        .uri
        .as_str()
        .contains(temp.path().to_string_lossy().as_ref()));

    let artifact_path = artifact_store
        .resolve_artifact_path(&execution.artifact)
        .unwrap();
    assert_eq!(
        artifact_path.extension().and_then(|value| value.to_str()),
        Some("svg")
    );
    assert!(std::fs::read_to_string(artifact_path)
        .unwrap()
        .starts_with("<svg "));
}

#[test]
fn preflight_blocks_missing_generate_capability_before_attempt_creation() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Preflight").unwrap();

    let mut plugin = generated_plugin();
    plugin
        .manifest
        .capabilities
        .retain(|value| value != "visual_generate");

    let preparation = preparation(None, false);
    let job = state
        .create_job(
            &project.id,
            "visual.generate",
            "SC01",
            &preparation.request.input_hash_v1().unwrap(),
        )
        .unwrap();
    let artifact_store = ArtifactStore::new(workspace.data_root()).unwrap();

    let error = omnicreator_core::execute_generated_image_plugin_v1(
        &mut state,
        &artifact_store,
        &plugin,
        temp.path().join("plugin-runtime"),
        &job.job_id,
        &preparation,
        PluginProcessOptions::default(),
    )
    .unwrap_err();

    assert!(error.to_string().contains("preflight blocked"));
    assert!(state.list_attempts(&job.job_id).unwrap().is_empty());
    assert_eq!(state.get_job(&job.job_id).unwrap().status, StepStatus::Ready);
}

#[test]
fn generated_image_retry_preserves_logical_job_and_appends_attempt_history() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Retry").unwrap();
    let request = request();
    let job = state
        .create_job(
            &project.id,
            "visual.generate",
            &request.scene.id,
            &request.input_hash_v1().unwrap(),
        )
        .unwrap();

    let first = state
        .start_attempt(&job.job_id, Some("plugin:generated-image-reference"))
        .unwrap();
    state
        .finish_attempt_failure(&first.attempt_id, "NETWORK_TIMEOUT")
        .unwrap();

    let second = state
        .start_attempt(&job.job_id, Some("plugin:generated-image-reference"))
        .unwrap();

    let attempts = state.list_attempts(&job.job_id).unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].status, StepStatus::Retryable);
    assert_eq!(attempts[1].status, StepStatus::Running);
    assert_eq!(attempts[0].job_id, job.job_id);
    assert_eq!(attempts[1].job_id, job.job_id);
    assert_ne!(first.attempt_id, second.attempt_id);
}

#[test]
fn generated_image_resource_declaration_is_phase7_scheduler_compatible() {
    let plugin = generated_plugin();
    let preparation = preparation(Some("fixture-gpu"), true);
    let job = omnicreator_core::Job {
        job_id: "job_image".to_owned(),
        project_id: "prj_image".to_owned(),
        step: "visual.generate".to_owned(),
        unit: "SC01".to_owned(),
        status: StepStatus::Ready,
        input_hash: preparation.request.input_hash_v1().unwrap(),
        selected_attempt: None,
        selected_artifact: None,
    };

    let gpu_preparation = preparation
        .to_gpu_job_preparation_v1(&job.job_id, &plugin)
        .unwrap();
    assert_eq!(
        gpu_preparation.requirements.model_group.as_deref(),
        Some("generated-image-reference-v1")
    );
    assert_eq!(
        gpu_preparation.requirements.cost_metric.as_deref(),
        Some("megapixels")
    );

    let now = Utc.with_ymd_and_hms(2026, 9, 5, 6, 0, 0).unwrap();
    let provider = ComputeProviderSchedulingSnapshotV1 {
        state: ComputeProviderConnectionState::Ready,
        session: ComputeProviderSessionV1 {
            identity: ComputeProviderSessionIdentityV1 {
                provider_id: "fixture-gpu".to_owned(),
                session_id: "session-image".to_owned(),
            },
            connected_at: now,
            last_heartbeat_at: now,
            last_healthy_heartbeat_at: Some(now),
            capabilities: ComputeProviderCapabilitiesV1 {
                schema: "omnicreator.compute-capabilities".to_owned(),
                schema_version: 1,
                provider_id: "fixture-gpu".to_owned(),
                devices: vec![
                    ComputeDeviceV1 {
                        id: "gpu0".to_owned(),
                        device_type: "gpu".to_owned(),
                        model: Some("NVIDIA T4".to_owned()),
                        memory_mb: Some(15_360),
                    },
                    ComputeDeviceV1 {
                        id: "gpu1".to_owned(),
                        device_type: "gpu".to_owned(),
                        model: Some("NVIDIA T4".to_owned()),
                        memory_mb: Some(15_360),
                    },
                ],
                model_groups: vec!["generated-image-reference-v1".to_owned()],
                max_parallel_jobs: Some(2),
            },
        },
    };
    let facts = GpuReadinessFactsV1 {
        workflow_step_status: Some(StepStatus::Ready),
        dependencies_succeeded: true,
        production_locked: true,
        cache_lookup: CacheLookupV1::Miss,
    };

    let decision =
        evaluate_gpu_queue(&job, &facts, &gpu_preparation, &[provider], &[]).unwrap();
    assert_eq!(decision.status, GpuQueueEligibilityStatusV1::GpuReady);
    assert_eq!(decision.selection.unwrap().device_id, "gpu0");
}
