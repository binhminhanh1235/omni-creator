use chrono::{DateTime, TimeZone, Utc};
use omnicreator_core::{
    ComputeAttemptRuntimeContextV1, ComputeProviderCapabilitiesV1,
    ComputeProviderConnectionState, ComputeProviderSchedulingSnapshotV1,
    ComputeProviderSessionIdentityV1, ComputeProviderSessionV1, ComputeRequirements,
    GpuBatchPlanRequestV1, GpuJobPreparationV1, GpuSerialBudgetSignalV1, LogicalUri,
    ResourceRequirement, StateStore, StepStatus, Workspace, GPU_WEEKLY_BUDGET_SCHEMA_V1,
};

fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, hour, minute, 0)
        .unwrap()
}

fn capabilities() -> ComputeProviderCapabilitiesV1 {
    ComputeProviderCapabilitiesV1::from_json_v1(include_str!(
        "fixtures/contracts/v1/compute-capabilities.json"
    ))
    .unwrap()
}

fn session(session_id: &str, connected_at: DateTime<Utc>) -> ComputeProviderSessionV1 {
    ComputeProviderSessionV1 {
        identity: ComputeProviderSessionIdentityV1 {
            provider_id: "kaggle-session".to_owned(),
            session_id: session_id.to_owned(),
        },
        connected_at,
        last_heartbeat_at: connected_at,
        last_healthy_heartbeat_at: Some(connected_at),
        capabilities: capabilities(),
    }
}

fn provider_snapshot() -> ComputeProviderSchedulingSnapshotV1 {
    let connected_at = at(2026, 9, 5, 0, 0);
    ComputeProviderSchedulingSnapshotV1 {
        state: ComputeProviderConnectionState::Ready,
        session: session("batch-session", connected_at),
    }
}

fn preparation(job_id: &str, unit: &str, model_version: &str) -> GpuJobPreparationV1 {
    GpuJobPreparationV1 {
        job_id: job_id.to_owned(),
        input_resolved: true,
        input_immutable: true,
        plugin_id: Some("omnivoice".to_owned()),
        provider_id: Some("kaggle-session".to_owned()),
        model_id: Some("omnivoice-v3".to_owned()),
        model_version: Some(model_version.to_owned()),
        settings_fingerprint: Some("settings-v1".to_owned()),
        output_uri: Some(LogicalUri::parse(&format!("project://audio/{unit}.wav")).unwrap()),
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
        .create_step(project_id, "tts", unit, StepStatus::Ready, Some(input_hash))
        .unwrap();
    state
        .create_job(project_id, "tts", unit, input_hash)
        .unwrap()
}

fn seed_runtime_estimate(
    state: &mut StateStore,
    project_id: &str,
    unit: &str,
    model_version: &str,
    runtime_seconds: f64,
) {
    let job = state
        .create_job(
            project_id,
            "history",
            unit,
            &format!("history-{unit}-{model_version}"),
        )
        .unwrap();
    let attempt = state
        .start_attempt(
            &job.job_id,
            Some("kaggle-session/history-session/gpu0"),
        )
        .unwrap();
    state
        .record_compute_attempt_runtime_context_v1(&ComputeAttemptRuntimeContextV1 {
            attempt_id: attempt.attempt_id.clone(),
            provider_id: "kaggle-session".to_owned(),
            session_id: "history-session".to_owned(),
            device_id: "gpu0".to_owned(),
            plugin_id: "omnivoice".to_owned(),
            model_id: "omnivoice-v3".to_owned(),
            model_version: model_version.to_owned(),
            runtime_observation_eligible: true,
        })
        .unwrap();
    state.finish_attempt_success(&attempt.attempt_id).unwrap();
    state
        .record_runtime_observation_v1(
            &attempt.attempt_id,
            runtime_seconds,
            at(2026, 9, 4, 10, 0),
        )
        .unwrap()
        .unwrap();
}

#[test]
fn weekly_budget_counts_session_wall_clock_with_boundary_clipping() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let state = StateStore::open(workspace.sqlite_path()).unwrap();

    state
        .set_gpu_weekly_budget_v1("kaggle-session", 30.0 * 60.0 * 60.0, at(2026, 8, 31, 0, 0))
        .unwrap();

    let crossing = session("crossing", at(2026, 8, 30, 23, 30));
    state.start_compute_session_usage_v1(&crossing).unwrap();
    state
        .finish_compute_session_usage_v1(
            "kaggle-session",
            "crossing",
            at(2026, 8, 31, 0, 30),
        )
        .unwrap();

    let closed = session("closed", at(2026, 9, 1, 10, 0));
    state.start_compute_session_usage_v1(&closed).unwrap();
    state
        .finish_compute_session_usage_v1(
            "kaggle-session",
            "closed",
            at(2026, 9, 1, 12, 0),
        )
        .unwrap();

    let open = session("open", at(2026, 9, 5, 0, 0));
    state.start_compute_session_usage_v1(&open).unwrap();

    let status = state
        .gpu_weekly_budget_status_v1(
            "kaggle-session",
            at(2026, 8, 31, 0, 0),
            at(2026, 9, 5, 1, 0),
        )
        .unwrap()
        .unwrap();

    assert_eq!(status.schema, GPU_WEEKLY_BUDGET_SCHEMA_V1);
    assert_eq!(status.version, 1);
    assert_eq!(status.open_sessions, 1);
    assert!((status.used_session_seconds - 12_600.0).abs() < f64::EPSILON);
    assert!(
        (status.remaining_session_seconds - (108_000.0 - 12_600.0)).abs()
            < f64::EPSILON
    );
    assert_eq!(status.overage_session_seconds, 0.0);
}

#[test]
fn session_usage_registration_and_finish_are_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let state = StateStore::open(workspace.sqlite_path()).unwrap();

    let active = session("session-idempotent", at(2026, 9, 5, 1, 0));
    let first = state.start_compute_session_usage_v1(&active).unwrap();
    let duplicate = state.start_compute_session_usage_v1(&active).unwrap();
    assert_eq!(first, duplicate);

    let finished_at = at(2026, 9, 5, 2, 0);
    let finished = state
        .finish_compute_session_usage_v1(
            "kaggle-session",
            "session-idempotent",
            finished_at,
        )
        .unwrap();
    let duplicate_finish = state
        .finish_compute_session_usage_v1(
            "kaggle-session",
            "session-idempotent",
            finished_at,
        )
        .unwrap();
    assert_eq!(finished, duplicate_finish);

    assert!(state
        .finish_compute_session_usage_v1(
            "kaggle-session",
            "session-idempotent",
            at(2026, 9, 5, 2, 1),
        )
        .is_err());

    let mut conflicting = active;
    conflicting.connected_at = at(2026, 9, 5, 1, 1);
    conflicting.last_heartbeat_at = conflicting.connected_at;
    conflicting.last_healthy_heartbeat_at = Some(conflicting.connected_at);
    assert!(state.start_compute_session_usage_v1(&conflicting).is_err());
}

#[test]
fn batch_workload_uses_runtime_ema_and_keeps_unknown_jobs_visible() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Batch Workload").unwrap();

    seed_runtime_estimate(&mut state, &project.id, "history-known", "3.2", 900.0);

    let known = create_tts_job(&state, &project.id, "S01", "known-input");
    let unknown = create_tts_job(&state, &project.id, "S02", "unknown-input");
    let plan = state
        .plan_gpu_batch_v1(
            &GpuBatchPlanRequestV1 {
                project_ids: vec![project.id.clone()],
                preparations: vec![
                    preparation(&known.job_id, "S01", "3.2"),
                    preparation(&unknown.job_id, "S02", "3.3"),
                ],
            },
            &[provider_snapshot()],
            &[],
        )
        .unwrap();

    let workload = state.estimate_gpu_batch_workload_v1(&plan).unwrap();
    assert_eq!(workload.total_jobs, 2);
    assert_eq!(workload.estimated_jobs, 1);
    assert_eq!(workload.unknown_jobs, 1);
    assert!((workload.estimated_runtime_seconds - 900.0).abs() < f64::EPSILON);
    assert_eq!(workload.lines.len(), 2);
    assert_eq!(
        workload
            .lines
            .iter()
            .filter(|line| line.per_job_seconds.is_none())
            .count(),
        1
    );
}

#[test]
fn budget_overview_is_indeterminate_until_all_ready_runtime_is_known() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Budget Signal").unwrap();

    state
        .set_gpu_weekly_budget_v1("kaggle-session", 3_600.0, at(2026, 8, 31, 0, 0))
        .unwrap();
    let consumed = session("used", at(2026, 9, 1, 10, 0));
    state.start_compute_session_usage_v1(&consumed).unwrap();
    state
        .finish_compute_session_usage_v1(
            "kaggle-session",
            "used",
            at(2026, 9, 1, 10, 30),
        )
        .unwrap();

    seed_runtime_estimate(&mut state, &project.id, "history-32", "3.2", 900.0);
    let known = create_tts_job(&state, &project.id, "S01", "known-input");
    let unknown = create_tts_job(&state, &project.id, "S02", "unknown-input");

    let request = GpuBatchPlanRequestV1 {
        project_ids: vec![project.id.clone()],
        preparations: vec![
            preparation(&known.job_id, "S01", "3.2"),
            preparation(&unknown.job_id, "S02", "3.3"),
        ],
    };
    let plan = state
        .plan_gpu_batch_v1(&request, &[provider_snapshot()], &[])
        .unwrap();

    let initial = state
        .assess_gpu_batch_budget_v1(
            &plan,
            "kaggle-session",
            at(2026, 8, 31, 0, 0),
            at(2026, 9, 5, 0, 0),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        initial.serial_budget_signal,
        GpuSerialBudgetSignalV1::IndeterminateUnknownRuntime
    );
    assert_eq!(initial.weekly_budget.remaining_session_seconds, 1_800.0);

    seed_runtime_estimate(&mut state, &project.id, "history-33", "3.3", 1_000.0);
    let complete = state
        .assess_gpu_batch_budget_v1(
            &plan,
            "kaggle-session",
            at(2026, 8, 31, 0, 0),
            at(2026, 9, 5, 0, 0),
        )
        .unwrap()
        .unwrap();

    assert_eq!(complete.workload.unknown_jobs, 0);
    assert_eq!(
        complete.serial_budget_signal,
        GpuSerialBudgetSignalV1::ExceedsKnownSerialEstimate
    );
    assert!((complete.workload.estimated_runtime_seconds - 1_900.0).abs() < f64::EPSILON);
}

#[test]
fn missing_budget_is_explicit_instead_of_inventing_an_allowance() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let state = StateStore::open(workspace.sqlite_path()).unwrap();

    assert!(state
        .gpu_weekly_budget_status_v1(
            "kaggle-session",
            at(2026, 8, 31, 0, 0),
            at(2026, 9, 5, 0, 0),
        )
        .unwrap()
        .is_none());
}

#[test]
fn weekly_budget_survives_reopen_inside_portable_state() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    {
        let state = StateStore::open(workspace.sqlite_path()).unwrap();
        state
            .set_gpu_weekly_budget_v1(
                "kaggle-session",
                108_000.0,
                at(2026, 9, 5, 0, 0),
            )
            .unwrap();
    }

    let state = StateStore::open(workspace.sqlite_path()).unwrap();
    let config = state
        .get_gpu_weekly_budget_v1("kaggle-session")
        .unwrap()
        .unwrap();
    assert_eq!(config.allowance_seconds, 108_000.0);
}

#[test]
fn gpu_budget_core_is_provider_neutral_and_contains_no_machine_paths() {
    let source = include_str!("../src/gpu_budget.rs").to_lowercase();
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
            "GPU budget core leaked provider/machine/infrastructure term {forbidden}"
        );
    }
}
