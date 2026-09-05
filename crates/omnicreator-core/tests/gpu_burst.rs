use chrono::{DateTime, Utc};
use omnicreator_core::{
    ComputeProviderCapabilitiesV1, ComputeProviderConnectionState,
    ComputeProviderSchedulingSnapshotV1, ComputeProviderSessionIdentityV1,
    ComputeProviderSessionV1, ComputeRequirements, GpuBatchPlanRequestV1,
    GpuBurstArtifactSyncPolicyV1, GpuBurstInteractionPolicyV1, GpuBurstRetryPolicyV1,
    GpuJobPreparationV1, LogicalUri, ResourceRequirement, StateStore, StepStatus, Workspace,
    GPU_BURST_PLAN_SCHEMA_V1,
};

fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-05T05:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn capabilities() -> ComputeProviderCapabilitiesV1 {
    ComputeProviderCapabilitiesV1::from_json_v1(include_str!(
        "fixtures/contracts/v1/compute-capabilities.json"
    ))
    .unwrap()
}

fn provider_snapshot() -> ComputeProviderSchedulingSnapshotV1 {
    ComputeProviderSchedulingSnapshotV1 {
        state: ComputeProviderConnectionState::Ready,
        session: ComputeProviderSessionV1 {
            identity: ComputeProviderSessionIdentityV1 {
                provider_id: "kaggle-session".to_owned(),
                session_id: "burst-session".to_owned(),
            },
            connected_at: fixed_time(),
            last_heartbeat_at: fixed_time(),
            last_healthy_heartbeat_at: Some(fixed_time()),
            capabilities: capabilities(),
        },
    }
}

fn preparation(
    job_id: &str,
    plugin_id: &str,
    model_id: &str,
    model_version: &str,
    model_group: &str,
    output_uri: &str,
) -> GpuJobPreparationV1 {
    GpuJobPreparationV1 {
        job_id: job_id.to_owned(),
        input_resolved: true,
        input_immutable: true,
        plugin_id: Some(plugin_id.to_owned()),
        provider_id: Some("kaggle-session".to_owned()),
        model_id: Some(model_id.to_owned()),
        model_version: Some(model_version.to_owned()),
        settings_fingerprint: Some("settings-v1".to_owned()),
        output_uri: Some(LogicalUri::parse(output_uri).unwrap()),
        approval_required: false,
        approval_complete: true,
        production_lock_required: false,
        preflight_required: true,
        preflight_complete: true,
        gpu_execution_requested: true,
        requirements: ComputeRequirements {
            gpu: ResourceRequirement::Required,
            min_vram_mb: Some(12_288),
            model_group: Some(model_group.to_owned()),
            parallelizable: true,
            cost_metric: Some("seconds".to_owned()),
        },
    }
}

fn create_job(
    state: &StateStore,
    project_id: &str,
    step: &str,
    unit: &str,
    input_hash: &str,
) -> omnicreator_core::Job {
    state
        .create_step(project_id, step, unit, StepStatus::Ready, Some(input_hash))
        .unwrap();
    state.create_job(project_id, step, unit, input_hash).unwrap()
}

fn voice_preparation(job_id: &str, unit: &str) -> GpuJobPreparationV1 {
    preparation(
        job_id,
        "omnivoice",
        "omnivoice-v3",
        "3.2",
        "omnivoice-v3.2",
        &format!("project://audio/{unit}.wav"),
    )
}

fn image_preparation(job_id: &str, unit: &str) -> GpuJobPreparationV1 {
    preparation(
        job_id,
        "image-provider",
        "flux",
        "schnell-1",
        "flux-schnell",
        &format!("project://visuals/{unit}.png"),
    )
}

#[test]
fn tts_heavy_batch_uses_two_independent_t4_devices_in_parallel() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("TTS Heavy").unwrap();

    let jobs = ["S01", "S02", "S03", "S04"]
        .iter()
        .map(|unit| create_job(&state, &project.id, "tts", unit, &format!("hash-{unit}")))
        .collect::<Vec<_>>();
    let preparations = jobs
        .iter()
        .map(|job| voice_preparation(&job.job_id, &job.unit))
        .collect::<Vec<_>>();

    let batch = state
        .plan_gpu_batch_v1(
            &GpuBatchPlanRequestV1 {
                project_ids: vec![project.id],
                preparations,
            },
            &[provider_snapshot()],
            &[],
        )
        .unwrap();
    assert_eq!(batch.ready_jobs.len(), 4);

    let first = state
        .plan_gpu_burst_v1(&batch, &[provider_snapshot()])
        .unwrap();
    let second = state
        .plan_gpu_burst_v1(&batch, &[provider_snapshot()])
        .unwrap();

    assert_eq!(first, second);
    assert_eq!(first.schema, GPU_BURST_PLAN_SCHEMA_V1);
    assert_eq!(first.schedule_hash.len(), 64);
    assert_eq!(first.scheduled_job_count(), 4);
    assert_eq!(first.wave_count(), 2);
    assert!(first.blocked.is_empty());

    for wave in &first.waves {
        assert_eq!(wave.assignments.len(), 2);
        let mut devices = wave
            .assignments
            .iter()
            .map(|assignment| assignment.selection.device_id.as_str())
            .collect::<Vec<_>>();
        devices.sort();
        assert_eq!(devices, vec!["gpu0", "gpu1"]);
        assert!(wave
            .assignments
            .iter()
            .all(|assignment| assignment.affinity.model_group == "omnivoice-v3.2"));
    }

    assert_eq!(first.devices.len(), 2);
    assert!(first
        .devices
        .iter()
        .all(|device| device.assignment_count == 2 && device.affinity_switches == 0));
}

#[test]
fn mixed_tts_and_image_jobs_can_fill_the_same_wave() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Mixed Burst").unwrap();

    let voice = create_job(&state, &project.id, "tts", "S01", "voice-hash");
    let image = create_job(&state, &project.id, "image", "scene-01", "image-hash");

    let batch = state
        .plan_gpu_batch_v1(
            &GpuBatchPlanRequestV1 {
                project_ids: vec![project.id],
                preparations: vec![
                    voice_preparation(&voice.job_id, &voice.unit),
                    image_preparation(&image.job_id, &image.unit),
                ],
            },
            &[provider_snapshot()],
            &[],
        )
        .unwrap();

    let burst = state
        .plan_gpu_burst_v1(&batch, &[provider_snapshot()])
        .unwrap();

    assert_eq!(burst.wave_count(), 1);
    assert_eq!(burst.waves[0].assignments.len(), 2);
    let mut groups = burst.waves[0]
        .assignments
        .iter()
        .map(|assignment| assignment.affinity.model_group.as_str())
        .collect::<Vec<_>>();
    groups.sort();
    assert_eq!(groups, vec!["flux-schnell", "omnivoice-v3.2"]);

    let mut devices = burst.waves[0]
        .assignments
        .iter()
        .map(|assignment| assignment.selection.device_id.as_str())
        .collect::<Vec<_>>();
    devices.sort();
    assert_eq!(devices, vec!["gpu0", "gpu1"]);
}

#[test]
fn affinity_groups_run_contiguously_and_device_switches_are_counted() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Affinity Runs").unwrap();

    let mut preparations = Vec::new();
    for unit in ["I01", "I02"] {
        let job = create_job(&state, &project.id, "image", unit, &format!("hash-{unit}"));
        preparations.push(image_preparation(&job.job_id, unit));
    }
    for unit in ["S01", "S02", "S03", "S04"] {
        let job = create_job(&state, &project.id, "tts", unit, &format!("hash-{unit}"));
        preparations.push(voice_preparation(&job.job_id, unit));
    }

    let batch = state
        .plan_gpu_batch_v1(
            &GpuBatchPlanRequestV1 {
                project_ids: vec![project.id],
                preparations,
            },
            &[provider_snapshot()],
            &[],
        )
        .unwrap();
    let burst = state
        .plan_gpu_burst_v1(&batch, &[provider_snapshot()])
        .unwrap();

    assert_eq!(burst.wave_count(), 3);
    let wave_groups = burst
        .waves
        .iter()
        .map(|wave| {
            let groups = wave
                .assignments
                .iter()
                .map(|assignment| assignment.affinity.model_group.as_str())
                .collect::<Vec<_>>();
            assert!(groups.windows(2).all(|pair| pair[0] == pair[1]));
            groups[0].to_owned()
        })
        .collect::<Vec<_>>();

    assert_eq!(
        wave_groups,
        vec![
            "flux-schnell".to_owned(),
            "omnivoice-v3.2".to_owned(),
            "omnivoice-v3.2".to_owned()
        ]
    );
    assert!(burst
        .devices
        .iter()
        .all(|device| device.assignment_count == 3 && device.affinity_switches == 1));
}

#[test]
fn burst_rechecks_current_state_and_blocks_stale_reviewed_job() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("State Drift").unwrap();
    let job = create_job(&state, &project.id, "image", "scene-01", "state-hash");

    let batch = state
        .plan_gpu_batch_v1(
            &GpuBatchPlanRequestV1 {
                project_ids: vec![project.id],
                preparations: vec![image_preparation(&job.job_id, &job.unit)],
            },
            &[provider_snapshot()],
            &[],
        )
        .unwrap();
    assert_eq!(batch.ready_jobs.len(), 1);

    state.start_attempt(&job.job_id, Some("other-worker")).unwrap();

    let burst = state
        .plan_gpu_burst_v1(&batch, &[provider_snapshot()])
        .unwrap();
    assert_eq!(burst.scheduled_job_count(), 0);
    assert_eq!(burst.blocked.len(), 1);
    assert!(burst.blocked[0]
        .decision
        .reasons
        .iter()
        .any(|reason| reason.code == omnicreator_core::GpuNotReadyReasonCodeV1::JobStateNotSchedulable));
}

#[test]
fn preflight_vram_failure_never_enters_burst_schedule() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("No VRAM Pooling").unwrap();
    let job = create_job(&state, &project.id, "image", "scene-01", "large-model-hash");
    let mut prep = image_preparation(&job.job_id, &job.unit);
    prep.requirements.min_vram_mb = Some(20_000);

    let batch = state
        .plan_gpu_batch_v1(
            &GpuBatchPlanRequestV1 {
                project_ids: vec![project.id],
                preparations: vec![prep],
            },
            &[provider_snapshot()],
            &[],
        )
        .unwrap();
    assert!(batch.ready_jobs.is_empty());
    assert_eq!(batch.blocked_jobs.len(), 1);

    let burst = state
        .plan_gpu_burst_v1(&batch, &[provider_snapshot()])
        .unwrap();
    assert_eq!(burst.scheduled_job_count(), 0);
    assert_eq!(burst.preflight_blocked_job_ids, vec![job.job_id]);
}

#[test]
fn burst_policy_is_non_interactive_error_aware_and_immediate_sync() {
    let policy = omnicreator_core::GpuBurstExecutionPolicyV1::default_v1();

    assert_eq!(policy.interaction, GpuBurstInteractionPolicyV1::NonInteractive);
    assert_eq!(policy.retry, GpuBurstRetryPolicyV1::ErrorAware);
    assert_eq!(
        policy.artifact_sync,
        GpuBurstArtifactSyncPolicyV1::ImmediateVerifiedLocalCommit
    );
    assert!(!policy.requires_human_prompt_after_start());
    assert!(policy.should_retry_error_v1("NETWORK_TIMEOUT"));
    assert!(policy.should_retry_error_v1("WORKER_LOST"));
    assert!(policy.should_retry_error_v1("MODEL_LOAD_ERROR"));
    assert!(policy.should_retry_error_v1("CUDA_OOM"));
    assert!(!policy.should_retry_error_v1("INVALID_INPUT"));
}

#[test]
fn gpu_burst_core_is_provider_neutral_and_portable() {
    let source = include_str!("../src/gpu_burst.rs").to_lowercase();
    for forbidden in [
        "kaggle.com",
        "notebook",
        "c:\\",
        "/home/",
        "/users/",
        "redis",
        "rabbitmq",
        "kafka",
        "kubernetes",
    ] {
        assert!(
            !source.contains(forbidden),
            "GPU burst core leaked provider/machine/infrastructure term {forbidden}"
        );
    }
}
