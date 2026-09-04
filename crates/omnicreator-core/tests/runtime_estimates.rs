use chrono::{TimeZone, Utc};
use omnicreator_core::{
    update_estimate_v1, ComputeAttemptRuntimeContextV1, RuntimeEstimateKeyV1,
    RuntimeWorkloadItemV1, StateStore, Workspace, RUNTIME_EMA_ALPHA_V1,
};

fn key() -> RuntimeEstimateKeyV1 {
    RuntimeEstimateKeyV1 {
        provider_id: "compute-provider".to_owned(),
        device_id: "gpu0".to_owned(),
        plugin_id: "omnivoice".to_owned(),
        model_id: "omnivoice-v3".to_owned(),
        model_version: "3.2".to_owned(),
    }
}

fn context(attempt_id: &str) -> ComputeAttemptRuntimeContextV1 {
    ComputeAttemptRuntimeContextV1 {
        attempt_id: attempt_id.to_owned(),
        provider_id: "compute-provider".to_owned(),
        session_id: "session-runtime".to_owned(),
        device_id: "gpu0".to_owned(),
        plugin_id: "omnivoice".to_owned(),
        model_id: "omnivoice-v3".to_owned(),
        model_version: "3.2".to_owned(),
        runtime_observation_eligible: true,
    }
}

#[test]
fn ema_and_mean_update_deterministically() {
    let first_at = Utc.with_ymd_and_hms(2026, 9, 4, 10, 0, 0).unwrap();
    let second_at = Utc.with_ymd_and_hms(2026, 9, 4, 10, 1, 0).unwrap();

    let first = update_estimate_v1(None, key(), 10.0, first_at).unwrap();
    assert_eq!(first.sample_count, 1);
    assert_eq!(first.mean_runtime_seconds, 10.0);
    assert_eq!(first.ema_runtime_seconds, 10.0);

    let second = update_estimate_v1(Some(&first), key(), 20.0, second_at).unwrap();
    let expected_ema = RUNTIME_EMA_ALPHA_V1 * 20.0 + (1.0 - RUNTIME_EMA_ALPHA_V1) * 10.0;

    assert_eq!(second.sample_count, 2);
    assert_eq!(second.total_runtime_seconds, 30.0);
    assert_eq!(second.mean_runtime_seconds, 15.0);
    assert!((second.ema_runtime_seconds - expected_ema).abs() < f64::EPSILON);
    assert_eq!(second.last_runtime_seconds, 20.0);
    assert_eq!(second.updated_at, second_at);
}

#[test]
fn persisted_runtime_observations_are_idempotent_and_feed_workload_projection() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Runtime Estimates").unwrap();

    let job1 = state
        .create_job(&project.id, "tts", "S01", "runtime-input-1")
        .unwrap();
    let attempt1 = state
        .start_attempt(&job1.job_id, Some("compute-provider/session-runtime/gpu0"))
        .unwrap();
    state
        .record_compute_attempt_runtime_context_v1(&context(&attempt1.attempt_id))
        .unwrap();
    state.finish_attempt_success(&attempt1.attempt_id).unwrap();

    let first_at = Utc.with_ymd_and_hms(2026, 9, 4, 10, 0, 0).unwrap();
    let first = state
        .record_runtime_observation_v1(&attempt1.attempt_id, 10.0, first_at)
        .unwrap()
        .unwrap();
    assert_eq!(first.sample_count, 1);

    let job2 = state
        .create_job(&project.id, "tts", "S02", "runtime-input-2")
        .unwrap();
    let attempt2 = state
        .start_attempt(&job2.job_id, Some("compute-provider/session-runtime/gpu0"))
        .unwrap();
    state
        .record_compute_attempt_runtime_context_v1(&context(&attempt2.attempt_id))
        .unwrap();
    state.finish_attempt_success(&attempt2.attempt_id).unwrap();

    let second_at = Utc.with_ymd_and_hms(2026, 9, 4, 10, 1, 0).unwrap();
    let second = state
        .record_runtime_observation_v1(&attempt2.attempt_id, 20.0, second_at)
        .unwrap()
        .unwrap();
    assert_eq!(second.sample_count, 2);
    assert_eq!(second.mean_runtime_seconds, 15.0);
    assert!((second.ema_runtime_seconds - 13.5).abs() < f64::EPSILON);

    let duplicate = state
        .record_runtime_observation_v1(&attempt2.attempt_id, 999.0, second_at)
        .unwrap()
        .unwrap();
    assert_eq!(duplicate.sample_count, 2);
    assert!((duplicate.ema_runtime_seconds - 13.5).abs() < f64::EPSILON);

    let unknown_key = RuntimeEstimateKeyV1 {
        device_id: "gpu1".to_owned(),
        ..key()
    };
    let workload = state
        .estimate_runtime_workload_v1(&[
            RuntimeWorkloadItemV1 {
                key: key(),
                job_count: 4,
            },
            RuntimeWorkloadItemV1 {
                key: unknown_key,
                job_count: 2,
            },
        ])
        .unwrap();

    assert_eq!(workload.total_jobs, 6);
    assert_eq!(workload.estimated_jobs, 4);
    assert_eq!(workload.unknown_jobs, 2);
    assert!((workload.estimated_runtime_seconds - 54.0).abs() < f64::EPSILON);
    assert_eq!(workload.lines.len(), 2);
    assert_eq!(workload.lines[0].sample_count, 2);
    assert_eq!(workload.lines[1].sample_count, 0);
    assert!(workload.lines[1].per_job_seconds.is_none());
}

#[test]
fn ineligible_context_is_not_learned() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Runtime Exclusion").unwrap();
    let job = state
        .create_job(&project.id, "tts", "S01", "runtime-input")
        .unwrap();
    let attempt = state
        .start_attempt(&job.job_id, Some("compute-provider/session-runtime/gpu0"))
        .unwrap();

    let mut runtime_context = context(&attempt.attempt_id);
    runtime_context.runtime_observation_eligible = false;
    state
        .record_compute_attempt_runtime_context_v1(&runtime_context)
        .unwrap();
    state.finish_attempt_success(&attempt.attempt_id).unwrap();

    let observed_at = Utc.with_ymd_and_hms(2026, 9, 4, 10, 0, 0).unwrap();
    assert!(state
        .record_runtime_observation_v1(&attempt.attempt_id, 10.0, observed_at)
        .unwrap()
        .is_none());
    assert!(state.get_runtime_estimate_v1(&key()).unwrap().is_none());
}
