use chrono::{DateTime, Utc};
use omnicreator_core::{
    evaluate_gpu_queue, Artifact, CacheLookupV1, ComputeProviderCapabilitiesV1,
    ComputeProviderConnectionState, ComputeProviderSchedulingSnapshotV1,
    ComputeProviderSessionIdentityV1, ComputeProviderSessionV1, ComputeRequirements,
    ComputeRunningAssignmentV1, GpuJobPreparationV1, GpuNotReadyReasonCodeV1,
    GpuQueueEligibilityStatusV1, GpuReadinessFactsV1, Job, LogicalUri, ResourceRequirement,
    StateStore, StepStatus, Workspace,
};
use serde_json::json;

fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-04T15:00:00Z")
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
                provider_id: "kaggle-session".to_owned(),
                session_id: "session-p1".to_owned(),
            },
            connected_at: fixed_time(),
            last_heartbeat_at: fixed_time(),
            last_healthy_heartbeat_at: Some(fixed_time()),
            capabilities: t4_capabilities(),
        },
    }
}

fn logical_job(id: &str) -> Job {
    Job {
        job_id: id.to_owned(),
        project_id: "prj_test".to_owned(),
        step: "tts".to_owned(),
        unit: "S01".to_owned(),
        status: StepStatus::Ready,
        input_hash: "input-hash-001".to_owned(),
        selected_attempt: None,
        selected_artifact: None,
    }
}

fn facts() -> GpuReadinessFactsV1 {
    GpuReadinessFactsV1 {
        workflow_step_status: Some(StepStatus::Ready),
        dependencies_succeeded: true,
        production_locked: true,
        cache_lookup: CacheLookupV1::Miss,
    }
}

fn preparation(job_id: &str) -> GpuJobPreparationV1 {
    GpuJobPreparationV1 {
        job_id: job_id.to_owned(),
        input_resolved: true,
        input_immutable: true,
        plugin_id: Some("omnivoice".to_owned()),
        provider_id: Some("kaggle-session".to_owned()),
        model_id: Some("omnivoice-v3".to_owned()),
        model_version: Some("3.2".to_owned()),
        settings_fingerprint: Some("settings-v1".to_owned()),
        output_uri: Some(LogicalUri::parse("project://audio/S01.wav").unwrap()),
        approval_required: true,
        approval_complete: true,
        production_lock_required: true,
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

fn codes(
    decision: &omnicreator_core::GpuQueueEligibilityV1,
) -> Vec<GpuNotReadyReasonCodeV1> {
    decision.reasons.iter().map(|reason| reason.code).collect()
}

fn running_from_ready(
    job_id: &str,
    decision: &omnicreator_core::GpuQueueEligibilityV1,
) -> ComputeRunningAssignmentV1 {
    let selection = decision.selection.as_ref().unwrap();
    ComputeRunningAssignmentV1 {
        job_id: job_id.to_owned(),
        provider_id: selection.provider_id.clone(),
        session_id: selection.session_id.clone(),
        device_id: selection.device_id.clone(),
        parallelizable: selection.parallelizable,
        parallelism_group: selection.parallelism_group.clone(),
    }
}

#[test]
fn fully_prepared_job_is_gpu_ready_and_selection_is_deterministic() {
    let job = logical_job("job-1");
    let prep = preparation(&job.job_id);
    let providers = vec![provider_snapshot()];

    let first = evaluate_gpu_queue(&job, &facts(), &prep, &providers, &[]).unwrap();
    let second = evaluate_gpu_queue(&job, &facts(), &prep, &providers, &[]).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.status, GpuQueueEligibilityStatusV1::GpuReady);
    assert!(first.reasons.is_empty());
    assert_eq!(first.selection.as_ref().unwrap().device_id, "gpu0");
    assert_eq!(first.selection.as_ref().unwrap().provider_id, "kaggle-session");
}

#[test]
fn dependency_and_input_hash_gates_block_gpu_ready() {
    let mut job = logical_job("job-1");
    job.input_hash.clear();
    let mut readiness = facts();
    readiness.dependencies_succeeded = false;

    let decision = evaluate_gpu_queue(
        &job,
        &readiness,
        &preparation(&job.job_id),
        &[provider_snapshot()],
        &[],
    )
    .unwrap();

    assert_eq!(decision.status, GpuQueueEligibilityStatusV1::NotReady);
    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::DependenciesIncomplete));
    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::InputHashMissing));
}

#[test]
fn unresolved_or_mutable_input_is_not_gpu_ready() {
    let job = logical_job("job-1");
    let mut prep = preparation(&job.job_id);
    prep.input_resolved = false;
    prep.input_immutable = false;

    let decision =
        evaluate_gpu_queue(&job, &facts(), &prep, &[provider_snapshot()], &[]).unwrap();

    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::InputNotResolved));
    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::InputNotImmutable));
}

#[test]
fn missing_execution_metadata_reports_all_not_ready_reasons() {
    let job = logical_job("job-1");
    let mut prep = preparation(&job.job_id);
    prep.plugin_id = None;
    prep.provider_id = None;
    prep.model_id = None;
    prep.model_version = None;
    prep.settings_fingerprint = None;
    prep.output_uri = None;

    let decision =
        evaluate_gpu_queue(&job, &facts(), &prep, &[provider_snapshot()], &[]).unwrap();
    let actual = codes(&decision);

    for expected in [
        GpuNotReadyReasonCodeV1::PluginUnknown,
        GpuNotReadyReasonCodeV1::ProviderUnknown,
        GpuNotReadyReasonCodeV1::ModelUnknown,
        GpuNotReadyReasonCodeV1::ModelVersionUnknown,
        GpuNotReadyReasonCodeV1::SettingsUnknown,
        GpuNotReadyReasonCodeV1::OutputUnknown,
    ] {
        assert!(actual.contains(&expected), "missing reason {expected:?}");
    }
}

#[test]
fn cache_not_checked_and_cache_hit_never_enter_gpu_queue() {
    let job = logical_job("job-1");
    let prep = preparation(&job.job_id);

    let mut not_checked = facts();
    not_checked.cache_lookup = CacheLookupV1::NotChecked;
    let decision = evaluate_gpu_queue(
        &job,
        &not_checked,
        &prep,
        &[provider_snapshot()],
        &[],
    )
    .unwrap();
    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::CacheNotChecked));

    let mut hit = facts();
    hit.cache_lookup = CacheLookupV1::Hit {
        artifact_id: "artifact-local".to_owned(),
    };
    let decision =
        evaluate_gpu_queue(&job, &hit, &prep, &[provider_snapshot()], &[]).unwrap();
    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::CacheHit));
    assert!(decision.selection.is_none());
}

#[test]
fn approval_lock_and_preflight_are_hard_gates_when_required() {
    let job = logical_job("job-1");
    let mut prep = preparation(&job.job_id);
    prep.approval_complete = false;
    prep.preflight_complete = false;
    let mut readiness = facts();
    readiness.production_locked = false;

    let decision = evaluate_gpu_queue(
        &job,
        &readiness,
        &prep,
        &[provider_snapshot()],
        &[],
    )
    .unwrap();

    let actual = codes(&decision);
    assert!(actual.contains(&GpuNotReadyReasonCodeV1::ApprovalPending));
    assert!(actual.contains(&GpuNotReadyReasonCodeV1::ProductionLockMissing));
    assert!(actual.contains(&GpuNotReadyReasonCodeV1::PreflightPending));
}

#[test]
fn gpu_required_optional_and_none_semantics_are_explicit() {
    let job = logical_job("job-1");
    let providers = vec![provider_snapshot()];

    let mut required = preparation(&job.job_id);
    required.gpu_execution_requested = false;
    let decision = evaluate_gpu_queue(&job, &facts(), &required, &providers, &[]).unwrap();
    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::GpuExecutionNotRequested));

    let mut optional = preparation(&job.job_id);
    optional.requirements.gpu = ResourceRequirement::Optional;
    optional.gpu_execution_requested = true;
    let decision = evaluate_gpu_queue(&job, &facts(), &optional, &providers, &[]).unwrap();
    assert!(decision.is_gpu_ready());

    optional.gpu_execution_requested = false;
    let decision = evaluate_gpu_queue(&job, &facts(), &optional, &providers, &[]).unwrap();
    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::GpuExecutionNotRequested));

    let mut cpu_only = preparation(&job.job_id);
    cpu_only.requirements.gpu = ResourceRequirement::None;
    cpu_only.requirements.min_vram_mb = None;
    cpu_only.requirements.model_group = None;
    let decision = evaluate_gpu_queue(&job, &facts(), &cpu_only, &providers, &[]).unwrap();
    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::GpuNotSupported));
}

#[test]
fn twelve_gb_job_rejects_device_with_insufficient_vram() {
    let job = logical_job("job-1");
    let prep = preparation(&job.job_id);
    let mut provider = provider_snapshot();
    for device in &mut provider.session.capabilities.devices {
        device.memory_mb = Some(8_192);
    }

    let decision = evaluate_gpu_queue(&job, &facts(), &prep, &[provider], &[]).unwrap();

    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::InsufficientVram));
}

#[test]
fn two_t4_devices_schedule_two_independent_parallel_jobs() {
    let provider = provider_snapshot();
    let providers = vec![provider];

    let job1 = logical_job("job-1");
    let first = evaluate_gpu_queue(
        &job1,
        &facts(),
        &preparation(&job1.job_id),
        &providers,
        &[],
    )
    .unwrap();
    assert_eq!(first.selection.as_ref().unwrap().device_id, "gpu0");

    let running = vec![running_from_ready(&job1.job_id, &first)];

    let job2 = logical_job("job-2");
    let second = evaluate_gpu_queue(
        &job2,
        &facts(),
        &preparation(&job2.job_id),
        &providers,
        &running,
    )
    .unwrap();

    assert_eq!(second.status, GpuQueueEligibilityStatusV1::GpuReady);
    assert_eq!(second.selection.as_ref().unwrap().device_id, "gpu1");
}

#[test]
fn scheduler_never_pools_memory_across_two_t4_devices() {
    let job = logical_job("job-1");
    let mut prep = preparation(&job.job_id);
    prep.requirements.min_vram_mb = Some(20_000);

    let decision =
        evaluate_gpu_queue(&job, &facts(), &prep, &[provider_snapshot()], &[]).unwrap();

    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::InsufficientVram));
    assert!(decision.selection.is_none());
}

#[test]
fn provider_max_parallel_jobs_is_respected() {
    let provider = provider_snapshot();
    let providers = vec![provider];

    let running = vec![
        ComputeRunningAssignmentV1 {
            job_id: "running-1".to_owned(),
            provider_id: "kaggle-session".to_owned(),
            session_id: "session-p1".to_owned(),
            device_id: "gpu0".to_owned(),
            parallelizable: true,
            parallelism_group: "group-a".to_owned(),
        },
        ComputeRunningAssignmentV1 {
            job_id: "running-2".to_owned(),
            provider_id: "kaggle-session".to_owned(),
            session_id: "session-p1".to_owned(),
            device_id: "gpu1".to_owned(),
            parallelizable: true,
            parallelism_group: "group-b".to_owned(),
        },
    ];

    let job = logical_job("job-3");
    let decision = evaluate_gpu_queue(
        &job,
        &facts(),
        &preparation(&job.job_id),
        &providers,
        &running,
    )
    .unwrap();

    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::ProviderAtCapacity));
}

#[test]
fn model_group_affinity_is_enforced() {
    let job = logical_job("job-1");
    let mut prep = preparation(&job.job_id);
    prep.requirements.model_group = Some("unknown-model-group".to_owned());

    let decision =
        evaluate_gpu_queue(&job, &facts(), &prep, &[provider_snapshot()], &[]).unwrap();

    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::ModelGroupUnsupported));
}

#[test]
fn non_parallelizable_group_cannot_run_concurrently() {
    let job1 = logical_job("job-1");
    let mut prep1 = preparation(&job1.job_id);
    prep1.requirements.parallelizable = false;
    let providers = vec![provider_snapshot()];

    let first = evaluate_gpu_queue(&job1, &facts(), &prep1, &providers, &[]).unwrap();
    assert!(first.is_gpu_ready());

    let running = vec![running_from_ready(&job1.job_id, &first)];

    let job2 = logical_job("job-2");
    let mut prep2 = preparation(&job2.job_id);
    prep2.requirements.parallelizable = false;
    let second = evaluate_gpu_queue(&job2, &facts(), &prep2, &providers, &running).unwrap();

    assert!(codes(&second).contains(&GpuNotReadyReasonCodeV1::ParallelismConflict));
}

#[test]
fn workflow_step_must_be_ready_or_retryable_to_enter_gpu_queue() {
    let job = logical_job("job-1");
    let prep = preparation(&job.job_id);
    let providers = vec![provider_snapshot()];

    for status in [
        StepStatus::NotReady,
        StepStatus::Queued,
        StepStatus::Running,
        StepStatus::Succeeded,
        StepStatus::Failed,
        StepStatus::Fatal,
        StepStatus::Stale,
        StepStatus::Skipped,
        StepStatus::Cancelled,
    ] {
        let mut readiness = facts();
        readiness.workflow_step_status = Some(status);

        let decision =
            evaluate_gpu_queue(&job, &readiness, &prep, &providers, &[]).unwrap();

        assert_eq!(decision.status, GpuQueueEligibilityStatusV1::NotReady);
        assert!(
            codes(&decision).contains(&GpuNotReadyReasonCodeV1::WorkflowStepNotReady),
            "workflow step {status:?} must not enter the GPU queue"
        );
        assert!(decision.selection.is_none());
    }

    for status in [StepStatus::Ready, StepStatus::Retryable] {
        let mut readiness = facts();
        readiness.workflow_step_status = Some(status);

        let decision =
            evaluate_gpu_queue(&job, &readiness, &prep, &providers, &[]).unwrap();

        assert!(
            decision.is_gpu_ready(),
            "workflow step {status:?} should be schedulable"
        );
    }
}

#[test]
fn unhealthy_stale_and_lost_sessions_cannot_claim_new_gpu_jobs() {
    let job = logical_job("job-1");
    let prep = preparation(&job.job_id);

    for state in [
        ComputeProviderConnectionState::Unhealthy,
        ComputeProviderConnectionState::Stale,
        ComputeProviderConnectionState::Lost,
    ] {
        let mut provider = provider_snapshot();
        provider.state = state;

        let decision =
            evaluate_gpu_queue(&job, &facts(), &prep, &[provider], &[]).unwrap();

        assert_eq!(decision.status, GpuQueueEligibilityStatusV1::NotReady);
        assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::ProviderNotReady));
        assert!(decision.selection.is_none());
    }
}

#[test]
fn ready_reconnect_session_wins_over_stale_session_for_same_provider() {
    let job = logical_job("job-1");
    let mut stale = provider_snapshot();
    stale.state = ComputeProviderConnectionState::Stale;
    stale.session.identity.session_id = "session-a-stale".to_owned();

    let mut ready = provider_snapshot();
    ready.session.identity.session_id = "session-b-ready".to_owned();

    let decision = evaluate_gpu_queue(
        &job,
        &facts(),
        &preparation(&job.job_id),
        &[stale, ready],
        &[],
    )
    .unwrap();

    assert!(decision.is_gpu_ready());
    assert_eq!(
        decision.selection.as_ref().unwrap().session_id,
        "session-b-ready"
    );
}

#[test]
fn missing_selected_provider_is_not_gpu_ready() {
    let job = logical_job("job-1");

    let decision =
        evaluate_gpu_queue(&job, &facts(), &preparation(&job.job_id), &[], &[]).unwrap();

    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::ProviderUnavailable));
    assert!(decision
        .reasons
        .iter()
        .all(|reason| !reason.message.trim().is_empty()));
}

#[test]
fn stale_provider_cannot_claim_new_gpu_job() {
    let job = logical_job("job-1");
    let mut provider = provider_snapshot();
    provider.state = ComputeProviderConnectionState::Stale;

    let decision = evaluate_gpu_queue(
        &job,
        &facts(),
        &preparation(&job.job_id),
        &[provider],
        &[],
    )
    .unwrap();

    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::ProviderNotReady));
}

#[test]
fn retryable_job_keeps_logical_job_identity() {
    let mut job = logical_job("logical-job");
    job.status = StepStatus::Retryable;

    let decision = evaluate_gpu_queue(
        &job,
        &facts(),
        &preparation(&job.job_id),
        &[provider_snapshot()],
        &[],
    )
    .unwrap();

    assert!(decision.is_gpu_ready());
    assert_eq!(decision.job_id, "logical-job");
}

#[test]
fn state_store_derives_dependency_readiness_and_cache_miss() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("GPU Ready State").unwrap();
    state
        .set_project_production_lock(&project.id, true)
        .unwrap();

    let upstream = state
        .create_step(
            &project.id,
            "script",
            "S01",
            StepStatus::Ready,
            Some("script-hash"),
        )
        .unwrap();
    let downstream = state
        .create_step(
            &project.id,
            "tts",
            "S01",
            StepStatus::Ready,
            Some("input-hash-001"),
        )
        .unwrap();
    state
        .add_dependency(&upstream.step_id, &downstream.step_id)
        .unwrap();

    let job = state
        .create_job(&project.id, "tts", "S01", "input-hash-001")
        .unwrap();
    let mut prep = preparation(&job.job_id);
    prep.provider_id = Some("kaggle-session".to_owned());

    let blocked = state
        .evaluate_gpu_queue(&prep, &[provider_snapshot()], &[])
        .unwrap();
    assert!(codes(&blocked).contains(&GpuNotReadyReasonCodeV1::DependenciesIncomplete));

    state
        .set_step_status(&upstream.step_id, StepStatus::Succeeded)
        .unwrap();

    let ready = state
        .evaluate_gpu_queue(&prep, &[provider_snapshot()], &[])
        .unwrap();
    assert!(ready.is_gpu_ready());
    assert_eq!(ready.job_id, job.job_id);
}

#[test]
fn state_store_cache_hit_prevents_remote_gpu_scheduling() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Cache Gate").unwrap();
    state
        .set_project_production_lock(&project.id, true)
        .unwrap();

    state
        .create_step(
            &project.id,
            "tts",
            "S02",
            StepStatus::Ready,
            Some("shared-input"),
        )
        .unwrap();

    let source_job = state
        .create_job(&project.id, "source", "S02", "shared-input")
        .unwrap();
    let artifact = Artifact {
        artifact_id: "artifact_cached".to_owned(),
        project_id: Some(project.id.clone()),
        artifact_type: "audio".to_owned(),
        uri: LogicalUri::parse("project://audio/cached.wav").unwrap(),
        sha256: "a".repeat(64),
        size_bytes: 42,
        input_hash: Some("shared-input".to_owned()),
        producer_job: Some(source_job.job_id),
        created_at: fixed_time(),
        metadata: json!({}),
    };
    state.commit_job_success(&artifact).unwrap();

    let target = state
        .create_job(&project.id, "tts", "S02", "shared-input")
        .unwrap();
    let mut prep = preparation(&target.job_id);
    prep.output_uri = Some(LogicalUri::parse("project://audio/new.wav").unwrap());

    let decision = state
        .evaluate_gpu_queue(&prep, &[provider_snapshot()], &[])
        .unwrap();

    assert!(codes(&decision).contains(&GpuNotReadyReasonCodeV1::CacheHit));
    assert!(decision.selection.is_none());
}

#[test]
fn invalid_gpu_none_resource_contract_is_rejected() {
    let job = logical_job("job-1");
    let mut prep = preparation(&job.job_id);
    prep.requirements.gpu = ResourceRequirement::None;

    assert!(
        evaluate_gpu_queue(&job, &facts(), &prep, &[provider_snapshot()], &[]).is_err(),
        "GPU-none requirements must not carry VRAM/model-group constraints"
    );
}
