use chrono::{DateTime, Utc};
use omnicreator_core::{
    ComputeProviderCapabilitiesV1, ComputeProviderConnectionState,
    ComputeProviderSchedulingSnapshotV1, ComputeProviderSessionIdentityV1,
    ComputeProviderSessionV1, ComputeRequirements, GpuBatchPlanRequestV1,
    GpuJobPreparationV1, GpuNotReadyReasonCodeV1, LogicalUri, ResourceRequirement,
    StateStore, StepStatus, Workspace, GPU_BATCH_PLAN_SCHEMA_V1,
};

fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-05T03:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn t4_capabilities() -> ComputeProviderCapabilitiesV1 {
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
                provider_id: "compute-provider".to_owned(),
                session_id: "batch-session".to_owned(),
            },
            connected_at: fixed_time(),
            last_heartbeat_at: fixed_time(),
            last_healthy_heartbeat_at: Some(fixed_time()),
            capabilities: t4_capabilities(),
        },
    }
}

fn preparation(job_id: &str, unit: &str) -> GpuJobPreparationV1 {
    GpuJobPreparationV1 {
        job_id: job_id.to_owned(),
        input_resolved: true,
        input_immutable: true,
        plugin_id: Some("voice-provider".to_owned()),
        provider_id: Some("compute-provider".to_owned()),
        model_id: Some("omnivoice-v3".to_owned()),
        model_version: Some("3.2".to_owned()),
        settings_fingerprint: Some("settings-v1".to_owned()),
        output_uri: Some(
            LogicalUri::parse(&format!("project://audio/{unit}.wav")).unwrap(),
        ),
        approval_required: false,
        approval_complete: true,
        production_lock_required: false,
        preflight_required: true,
        preflight_complete: true,
        gpu_execution_requested: true,
        requirements: ComputeRequirements {
            gpu: ResourceRequirement::Required,
            min_vram_mb: Some(12_288),
            model_group: Some("omnivoice-v3.2".to_owned()),
            parallelizable: true,
            cost_metric: Some("seconds".to_owned()),
        },
    }
}

fn create_tts_job(
    state: &StateStore,
    project_id: &str,
    unit: &str,
    input_hash: &str,
) -> omnicreator_core::Job {
    state
        .create_step(
            project_id,
            "tts",
            unit,
            StepStatus::Ready,
            Some(input_hash),
        )
        .unwrap();
    state
        .create_job(project_id, "tts", unit, input_hash)
        .unwrap()
}

fn reason_codes(job: &omnicreator_core::GpuBatchJobV1) -> Vec<GpuNotReadyReasonCodeV1> {
    job.eligibility
        .reasons
        .iter()
        .map(|reason| reason.code)
        .collect()
}

#[test]
fn multi_project_plan_is_deterministic_and_non_mutating() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let state = StateStore::open(workspace.sqlite_path()).unwrap();

    let project_a = state.create_project("Project A").unwrap();
    let project_b = state.create_project("Project B").unwrap();

    let job_a1 = create_tts_job(&state, &project_a.id, "S01", "hash-a1");
    let job_a2 = create_tts_job(&state, &project_a.id, "S02", "hash-a2");
    let job_b1 = create_tts_job(&state, &project_b.id, "S01", "hash-b1");

    let mut blocked = preparation(&job_a2.job_id, "S02");
    blocked.preflight_complete = false;

    let request_a = GpuBatchPlanRequestV1 {
        project_ids: vec![
            project_b.id.clone(),
            project_a.id.clone(),
            project_b.id.clone(),
        ],
        preparations: vec![
            preparation(&job_b1.job_id, "S01"),
            blocked.clone(),
            preparation(&job_a1.job_id, "S01"),
        ],
    };
    let request_b = GpuBatchPlanRequestV1 {
        project_ids: vec![project_a.id.clone(), project_b.id.clone()],
        preparations: vec![
            preparation(&job_a1.job_id, "S01"),
            preparation(&job_b1.job_id, "S01"),
            blocked,
        ],
    };

    let providers = vec![provider_snapshot()];
    let first = state
        .plan_gpu_batch_v1(&request_a, &providers, &[])
        .unwrap();
    let second = state
        .plan_gpu_batch_v1(&request_b, &providers, &[])
        .unwrap();

    assert_eq!(first, second);
    assert_eq!(first.schema, GPU_BATCH_PLAN_SCHEMA_V1);
    assert_eq!(first.version, 1);
    assert_eq!(first.snapshot_hash.len(), 64);
    assert_eq!(first.candidate_jobs, 3);
    assert_eq!(first.ready_jobs.len(), 2);
    assert_eq!(first.blocked_jobs.len(), 1);
    assert!(!first.is_ready_to_start());

    let mut expected_projects = vec![project_a.id.clone(), project_b.id.clone()];
    expected_projects.sort();
    assert_eq!(first.selected_project_ids, expected_projects);

    assert_eq!(first.project_summaries.len(), 2);
    assert_eq!(
        first
            .project_summaries
            .iter()
            .map(|summary| summary.candidate_jobs)
            .sum::<u64>(),
        3
    );
    assert_eq!(
        first
            .project_summaries
            .iter()
            .map(|summary| summary.ready_jobs)
            .sum::<u64>(),
        2
    );
    assert_eq!(
        first
            .project_summaries
            .iter()
            .map(|summary| summary.blocked_jobs)
            .sum::<u64>(),
        1
    );

    assert_eq!(first.work_kind_summaries.len(), 1);
    assert_eq!(first.work_kind_summaries[0].step, "tts");
    assert_eq!(
        first.work_kind_summaries[0].plugin_id.as_deref(),
        Some("voice-provider")
    );
    assert_eq!(first.work_kind_summaries[0].candidate_jobs, 3);
    assert_eq!(first.model_group_summaries.len(), 1);
    assert_eq!(
        first.model_group_summaries[0].model_group.as_deref(),
        Some("omnivoice-v3.2")
    );
    assert_eq!(first.model_group_summaries[0].ready_jobs, 2);
    assert_eq!(first.model_group_summaries[0].blocked_jobs, 1);

    let blocked_job = &first.blocked_jobs[0];
    assert_eq!(blocked_job.job_id, job_a2.job_id);
    assert!(
        reason_codes(blocked_job).contains(&GpuNotReadyReasonCodeV1::PreflightPending)
    );

    for job_id in [&job_a1.job_id, &job_a2.job_id, &job_b1.job_id] {
        assert_eq!(state.get_job(job_id).unwrap().status, StepStatus::Ready);
    }
}

#[test]
fn canonical_provider_failures_are_preserved_in_batch_preflight() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Provider Missing").unwrap();
    let job = create_tts_job(&state, &project.id, "S01", "hash-provider");

    let plan = state
        .plan_gpu_batch_v1(
            &GpuBatchPlanRequestV1 {
                project_ids: vec![project.id],
                preparations: vec![preparation(&job.job_id, "S01")],
            },
            &[],
            &[],
        )
        .unwrap();

    assert!(plan.ready_jobs.is_empty());
    assert_eq!(plan.blocked_jobs.len(), 1);
    assert!(
        reason_codes(&plan.blocked_jobs[0])
            .contains(&GpuNotReadyReasonCodeV1::ProviderUnavailable)
    );
}

#[test]
fn batch_rejects_duplicate_preparations_and_unselected_project_jobs() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let state = StateStore::open(workspace.sqlite_path()).unwrap();
    let selected = state.create_project("Selected").unwrap();
    let other = state.create_project("Other").unwrap();
    let selected_job = create_tts_job(&state, &selected.id, "S01", "hash-selected");
    let other_job = create_tts_job(&state, &other.id, "S01", "hash-other");

    let duplicate = state.plan_gpu_batch_v1(
        &GpuBatchPlanRequestV1 {
            project_ids: vec![selected.id.clone()],
            preparations: vec![
                preparation(&selected_job.job_id, "S01"),
                preparation(&selected_job.job_id, "S01"),
            ],
        },
        &[provider_snapshot()],
        &[],
    );
    assert!(duplicate.is_err());

    let unselected = state.plan_gpu_batch_v1(
        &GpuBatchPlanRequestV1 {
            project_ids: vec![selected.id],
            preparations: vec![preparation(&other_job.job_id, "S01")],
        },
        &[provider_snapshot()],
        &[],
    );
    assert!(unselected.is_err());
}

#[test]
fn candidate_job_discovery_is_sorted_across_selected_projects() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project_a = state.create_project("A").unwrap();
    let project_b = state.create_project("B").unwrap();

    let _a2 = create_tts_job(&state, &project_a.id, "S02", "hash-a2");
    let _a1 = create_tts_job(&state, &project_a.id, "S01", "hash-a1");
    let _b1 = create_tts_job(&state, &project_b.id, "S01", "hash-b1");

    let jobs = state
        .list_gpu_batch_candidate_jobs_v1(&[
            project_b.id.clone(),
            project_a.id.clone(),
            project_b.id.clone(),
        ])
        .unwrap();

    assert_eq!(jobs.len(), 3);
    let keys = jobs
        .iter()
        .map(|job| {
            (
                job.project_id.clone(),
                job.step.clone(),
                job.unit.clone(),
                job.job_id.clone(),
            )
        })
        .collect::<Vec<_>>();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[test]
fn empty_project_selection_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let state = StateStore::open(workspace.sqlite_path()).unwrap();

    assert!(state
        .plan_gpu_batch_v1(
            &GpuBatchPlanRequestV1 {
                project_ids: Vec::new(),
                preparations: Vec::new(),
            },
            &[],
            &[],
        )
        .is_err());
}

#[test]
fn gpu_batch_core_is_provider_neutral_and_portable() {
    let source = include_str!("../src/gpu_batch.rs").to_lowercase();
    for forbidden in [
        "kaggle",
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
            "GPU batch core leaked provider/machine/infrastructure term {forbidden}"
        );
    }
}
