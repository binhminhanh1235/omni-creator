use std::{collections::BTreeMap, path::PathBuf};

use chrono::{DateTime, Utc};
use omnicreator_core::{
    scan_plugin_roots, ComputeProviderCapabilitiesV1, ComputeProviderConnectionState,
    ComputeProviderSchedulingSnapshotV1, ComputeProviderSessionIdentityV1,
    ComputeProviderSessionV1, GeneratedImagePreparationV1, GeneratedImageRequestV1,
    GeneratedImageResolutionV1, GeneratedImageStyleV1, GpuBatchPlanRequestV1,
    GpuNotReadyReasonCodeV1, LogicalUri, ResourceRequirement, SceneIntentV1, StateStore,
    StepStatus, Workspace, SCENE_INTENT_SCHEMA, SCENE_INTENT_SCHEMA_VERSION,
};

fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-05T09:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

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

fn generated_capabilities() -> ComputeProviderCapabilitiesV1 {
    ComputeProviderCapabilitiesV1::from_json_v1(
        r#"{
          "schema": "omnicreator.compute-capabilities",
          "schema_version": 1,
          "provider_id": "compute-provider",
          "devices": [
            {
              "id": "gpu0",
              "device_type": "gpu",
              "model": "NVIDIA T4",
              "memory_mb": 15360
            },
            {
              "id": "gpu1",
              "device_type": "gpu",
              "model": "NVIDIA T4",
              "memory_mb": 15360
            }
          ],
          "model_groups": ["generated-image-reference-v1"],
          "max_parallel_jobs": 2
        }"#,
    )
    .unwrap()
}

fn provider_snapshot() -> ComputeProviderSchedulingSnapshotV1 {
    ComputeProviderSchedulingSnapshotV1 {
        state: ComputeProviderConnectionState::Ready,
        session: ComputeProviderSessionV1 {
            identity: ComputeProviderSessionIdentityV1 {
                provider_id: "compute-provider".to_owned(),
                session_id: "generated-image-session".to_owned(),
            },
            connected_at: fixed_time(),
            last_heartbeat_at: fixed_time(),
            last_healthy_heartbeat_at: Some(fixed_time()),
            capabilities: generated_capabilities(),
        },
    }
}

fn scene(scene_id: &str) -> SceneIntentV1 {
    SceneIntentV1 {
        schema: SCENE_INTENT_SCHEMA.to_owned(),
        schema_version: SCENE_INTENT_SCHEMA_VERSION,
        id: scene_id.to_owned(),
        segment_id: format!("SEG-{scene_id}"),
        narration: format!("A production-ready generated still for {scene_id}."),
        purpose: "Exercise canonical generated-image GPU planning.".to_owned(),
        scene_type: "conceptual".to_owned(),
        emotion_before: None,
        emotion_after: None,
        duration_hint: Some(5.0),
        visual_ideas: vec!["clean cinematic composition".to_owned()],
        search_queries: vec!["cinematic generated still".to_owned()],
        avoid: vec!["logos".to_owned(), "text overlays".to_owned()],
        continuity: BTreeMap::new(),
        aspect_ratio: "16:9".to_owned(),
    }
}

fn preparation(scene_id: &str) -> GeneratedImagePreparationV1 {
    GeneratedImagePreparationV1 {
        request: GeneratedImageRequestV1::from_scene_v1(
            scene(scene_id),
            GeneratedImageStyleV1 {
                preset: "workbench-hardening".to_owned(),
                description: Some("restrained cinematic still".to_owned()),
            },
            GeneratedImageResolutionV1 {
                width: 1280,
                height: 720,
            },
            Some(17),
            BTreeMap::new(),
        )
        .unwrap(),
        output_uri: Some(
            LogicalUri::parse(&format!("project://visual/{scene_id}.png")).unwrap(),
        ),
        provider_id: Some("compute-provider".to_owned()),
        model_id: Some("reference-svg".to_owned()),
        model_version: Some("1".to_owned()),
        approval_required: false,
        approval_complete: true,
        production_lock_required: false,
        gpu_execution_requested: true,
    }
}

fn create_generated_job(
    state: &StateStore,
    project_id: &str,
    preparation: &GeneratedImagePreparationV1,
) -> omnicreator_core::Job {
    let unit = preparation.request.scene.id.as_str();
    let input_hash = preparation.request.input_hash_v1().unwrap();
    state
        .create_step(
            project_id,
            "visual.generate",
            unit,
            StepStatus::Ready,
            Some(&input_hash),
        )
        .unwrap();
    state
        .create_job(project_id, "visual.generate", unit, &input_hash)
        .unwrap()
}

#[test]
fn generated_image_manifest_resources_flow_through_batch_workload_and_two_device_burst() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Generated Image Workbench").unwrap();
    let plugin = generated_plugin();

    let preparations = ["SC01", "SC02"]
        .into_iter()
        .map(|scene_id| {
            let generated = preparation(scene_id);
            let job = create_generated_job(&state, &project.id, &generated);
            let gpu = generated
                .to_gpu_job_preparation_v1(&job.job_id, &plugin)
                .unwrap();
            (job, gpu)
        })
        .collect::<Vec<_>>();

    for (_, gpu) in &preparations {
        assert_eq!(gpu.plugin_id.as_deref(), Some("generated-image-reference"));
        assert_eq!(gpu.provider_id.as_deref(), Some("compute-provider"));
        assert_eq!(gpu.requirements.gpu, ResourceRequirement::Optional);
        assert_eq!(gpu.requirements.min_vram_mb, Some(1024));
        assert_eq!(
            gpu.requirements.model_group.as_deref(),
            Some("generated-image-reference-v1")
        );
        assert_eq!(gpu.requirements.cost_metric.as_deref(), Some("megapixels"));
        assert!(gpu.requirements.parallelizable);
        assert!(gpu.preflight_complete);
        assert!(gpu.gpu_execution_requested);
    }

    let batch = state
        .plan_gpu_batch_v1(
            &GpuBatchPlanRequestV1 {
                project_ids: vec![project.id.clone()],
                preparations: preparations
                    .iter()
                    .map(|(_, gpu)| gpu.clone())
                    .collect(),
            },
            &[provider_snapshot()],
            &[],
        )
        .unwrap();

    assert_eq!(batch.candidate_jobs, 2);
    assert_eq!(batch.ready_jobs.len(), 2);
    assert!(batch.blocked_jobs.is_empty());
    assert!(batch.is_ready_to_start());
    assert_eq!(batch.work_kind_summaries.len(), 1);
    assert_eq!(batch.work_kind_summaries[0].step, "visual.generate");
    assert_eq!(
        batch.work_kind_summaries[0].plugin_id.as_deref(),
        Some("generated-image-reference")
    );
    assert_eq!(batch.model_group_summaries.len(), 1);
    assert_eq!(
        batch.model_group_summaries[0].provider_id.as_deref(),
        Some("compute-provider")
    );
    assert_eq!(
        batch.model_group_summaries[0].model_group.as_deref(),
        Some("generated-image-reference-v1")
    );
    assert_eq!(batch.model_group_summaries[0].ready_jobs, 2);

    let workload = state.estimate_gpu_batch_workload_v1(&batch).unwrap();
    assert_eq!(workload.total_jobs, 2);
    assert_eq!(workload.estimated_jobs, 0);
    assert_eq!(workload.unknown_jobs, 2);
    assert_eq!(
        workload.lines.iter().map(|line| line.job_count).sum::<u64>(),
        2
    );
    assert!(workload.lines.iter().all(|line| {
        line.key.provider_id == "compute-provider"
            && line.key.plugin_id == "generated-image-reference"
            && line.key.model_id == "reference-svg"
            && line.key.model_version == "1"
    }));

    let burst = state
        .plan_gpu_burst_v1(&batch, &[provider_snapshot()])
        .unwrap();
    assert_eq!(burst.wave_count(), 1);
    assert_eq!(burst.scheduled_job_count(), 2);
    assert!(burst.blocked.is_empty());
    assert_eq!(burst.waves[0].assignments.len(), 2);

    let mut devices = burst.waves[0]
        .assignments
        .iter()
        .map(|assignment| assignment.selection.device_id.as_str())
        .collect::<Vec<_>>();
    devices.sort();
    assert_eq!(devices, vec!["gpu0", "gpu1"]);
    assert!(burst.waves[0].assignments.iter().all(|assignment| {
        assignment.affinity.provider_id == "compute-provider"
            && assignment.affinity.plugin_id == "generated-image-reference"
            && assignment.affinity.model_group == "generated-image-reference-v1"
    }));

    for (job, _) in preparations {
        assert_eq!(state.get_job(&job.job_id).unwrap().status, StepStatus::Ready);
    }
}

#[test]
fn generated_image_vram_requirement_is_checked_per_device_without_pooling() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Generated Image VRAM Gate").unwrap();
    let plugin = generated_plugin();
    let generated = preparation("SC-LARGE");
    let job = create_generated_job(&state, &project.id, &generated);
    let mut gpu = generated
        .to_gpu_job_preparation_v1(&job.job_id, &plugin)
        .unwrap();

    // Each fixture GPU has 15,360 MiB. Two devices together exceed 20,000 MiB,
    // but canonical scheduling must never pool their VRAM for one job.
    gpu.requirements.min_vram_mb = Some(20_000);

    let batch = state
        .plan_gpu_batch_v1(
            &GpuBatchPlanRequestV1 {
                project_ids: vec![project.id],
                preparations: vec![gpu],
            },
            &[provider_snapshot()],
            &[],
        )
        .unwrap();

    assert!(batch.ready_jobs.is_empty());
    assert_eq!(batch.blocked_jobs.len(), 1);
    assert!(batch.blocked_jobs[0]
        .eligibility
        .reasons
        .iter()
        .any(|reason| reason.code == GpuNotReadyReasonCodeV1::InsufficientVram));
}

#[test]
fn generated_image_compute_workbench_bridge_is_provider_neutral_and_infrastructure_free() {
    let sources = [
        include_str!("../src/generated_image_remote.rs"),
        include_str!("../src/gpu_workbench.rs"),
    ];

    for source in sources {
        let lower = source.to_lowercase();
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
                !lower.contains(forbidden),
                "generated-image Workbench bridge leaked provider/machine/infrastructure term {forbidden}"
            );
        }
    }
}
