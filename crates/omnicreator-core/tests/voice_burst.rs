use chrono::{DateTime, Utc};
use omnicreator_core::{
    plan_voice_burst_v1, CacheLookupV1, ComputeProviderCapabilitiesV1,
    ComputeProviderConnectionState, ComputeProviderSchedulingSnapshotV1,
    ComputeProviderSessionIdentityV1, ComputeProviderSessionV1, GpuNotReadyReasonCodeV1,
    GpuReadinessFactsV1, Job, LogicalUri, SegmentTtsExecutionTargetV1, SegmentTtsLockStateV1,
    SegmentTtsPreparationV1, SegmentTtsProductionInputV1, SegmentV1, StepStatus,
    VoiceBurstCandidateV1, VoiceDirectionV1, VoiceIdentityV1, VoiceModelIdentityV1, SEGMENT_SCHEMA,
    SEGMENT_SCHEMA_VERSION,
};

fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-04T17:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn provider_snapshot(max_parallel_jobs: u32) -> ComputeProviderSchedulingSnapshotV1 {
    let mut capabilities = ComputeProviderCapabilitiesV1::from_json_v1(include_str!(
        "fixtures/contracts/v1/compute-capabilities.json"
    ))
    .unwrap();
    capabilities.provider_id = "compute-provider".to_owned();
    capabilities.model_groups = vec!["voice-model-a".to_owned(), "voice-model-b".to_owned()];
    capabilities.max_parallel_jobs = Some(max_parallel_jobs);

    ComputeProviderSchedulingSnapshotV1 {
        state: ComputeProviderConnectionState::Ready,
        session: ComputeProviderSessionV1 {
            identity: ComputeProviderSessionIdentityV1 {
                provider_id: "compute-provider".to_owned(),
                session_id: "session-burst-p1".to_owned(),
            },
            connected_at: fixed_time(),
            last_heartbeat_at: fixed_time(),
            last_healthy_heartbeat_at: Some(fixed_time()),
            capabilities,
        },
    }
}

fn tts_preparation(
    segment_id: &str,
    model_id: &str,
    model_version: &str,
    model_group: &str,
    voice_id: &str,
    voice_version: &str,
    min_vram_mb: u64,
    parallelizable: bool,
) -> SegmentTtsPreparationV1 {
    let segment = SegmentV1 {
        schema: SEGMENT_SCHEMA.to_owned(),
        schema_version: SEGMENT_SCHEMA_VERSION,
        id: segment_id.to_owned(),
        order: segment_id
            .trim_start_matches('S')
            .parse::<u32>()
            .unwrap_or(1),
        text: format!("Narration for {segment_id}."),
        voice_direction: VoiceDirectionV1 {
            tone: Some("warm".to_owned()),
            pace: Some("measured".to_owned()),
            tags: vec!["reflective".to_owned()],
        },
    };

    let production_input = SegmentTtsProductionInputV1::from_segment_v1(
        &segment,
        Vec::new(),
        VoiceIdentityV1 {
            voice_id: voice_id.to_owned(),
            voice_version: voice_version.to_owned(),
        },
        VoiceModelIdentityV1 {
            model_id: model_id.to_owned(),
            model_version: model_version.to_owned(),
        },
        "settings-v1",
    );

    let mut requirements =
        omnicreator_core::default_segment_tts_compute_requirements_v1(model_group, min_vram_mb);
    requirements.parallelizable = parallelizable;

    SegmentTtsPreparationV1 {
        segment_id: segment_id.to_owned(),
        production_input,
        locks: SegmentTtsLockStateV1 {
            normalization_locked: true,
            pronunciation_locked: true,
        },
        execution: SegmentTtsExecutionTargetV1 {
            plugin_id: Some("voice-provider".to_owned()),
            provider_id: Some("compute-provider".to_owned()),
            output_uri: Some(
                LogicalUri::parse(&format!("project://audio/{segment_id}.wav")).unwrap(),
            ),
            approval_required: false,
            approval_complete: true,
            production_lock_required: true,
            gpu_execution_requested: true,
            requirements,
        },
    }
}

fn readiness() -> GpuReadinessFactsV1 {
    GpuReadinessFactsV1 {
        workflow_step_status: Some(StepStatus::Ready),
        dependencies_succeeded: true,
        production_locked: true,
        cache_lookup: CacheLookupV1::Miss,
    }
}

fn candidate(
    job_id: &str,
    project_id: &str,
    segment_id: &str,
    model_id: &str,
    model_version: &str,
    model_group: &str,
    voice_id: &str,
    voice_version: &str,
    min_vram_mb: u64,
    parallelizable: bool,
) -> VoiceBurstCandidateV1 {
    let tts = tts_preparation(
        segment_id,
        model_id,
        model_version,
        model_group,
        voice_id,
        voice_version,
        min_vram_mb,
        parallelizable,
    );
    let input_hash = tts.input_hash_v1().unwrap();

    VoiceBurstCandidateV1 {
        job: Job {
            job_id: job_id.to_owned(),
            project_id: project_id.to_owned(),
            step: "tts".to_owned(),
            unit: segment_id.to_owned(),
            status: StepStatus::Ready,
            input_hash,
            selected_attempt: None,
            selected_artifact: None,
        },
        readiness: readiness(),
        tts,
    }
}

fn default_candidate(job_id: &str, segment_id: &str, voice_id: &str) -> VoiceBurstCandidateV1 {
    candidate(
        job_id,
        "prj-a",
        segment_id,
        "voice-model",
        "1.0",
        "voice-model-a",
        voice_id,
        "v1",
        12_288,
        true,
    )
}

#[test]
fn burst_plan_is_deterministic_and_groups_model_voice_affinity() {
    let original = vec![
        default_candidate("job-03", "S03", "voice-b"),
        default_candidate("job-02", "S02", "voice-a"),
        default_candidate("job-01", "S01", "voice-a"),
    ];
    let mut reversed = original.clone();
    reversed.reverse();

    let providers = vec![provider_snapshot(2)];
    let first = plan_voice_burst_v1(&original, &providers).unwrap();
    let second = plan_voice_burst_v1(&reversed, &providers).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.scheduled_job_count(), 3);
    assert!(first.blocked.is_empty());
    assert_eq!(first.waves.len(), 2);

    let wave0 = &first.waves[0];
    assert_eq!(
        wave0
            .assignments
            .iter()
            .map(|assignment| assignment.job_id.as_str())
            .collect::<Vec<_>>(),
        vec!["job-01", "job-02"]
    );
    assert!(wave0
        .assignments
        .iter()
        .all(|assignment| assignment.affinity.voice_id == "voice-a"));
    assert_eq!(wave0.assignments[0].selection.device_id, "gpu0");
    assert_eq!(wave0.assignments[1].selection.device_id, "gpu1");

    assert_eq!(first.waves[1].assignments[0].job_id, "job-03");
    assert_eq!(first.waves[1].assignments[0].affinity.voice_id, "voice-b");
}

#[test]
fn two_t4_devices_run_two_independent_segments_in_the_same_wave() {
    let candidates = vec![
        default_candidate("job-01", "S01", "voice-a"),
        default_candidate("job-02", "S02", "voice-a"),
    ];
    let plan = plan_voice_burst_v1(&candidates, &[provider_snapshot(2)]).unwrap();

    assert_eq!(plan.waves.len(), 1);
    assert_eq!(plan.waves[0].assignments.len(), 2);
    assert_eq!(plan.waves[0].assignments[0].selection.device_id, "gpu0");
    assert_eq!(plan.waves[0].assignments[1].selection.device_id, "gpu1");
    assert_ne!(
        plan.waves[0].assignments[0].selection.device_id,
        plan.waves[0].assignments[1].selection.device_id
    );
}

#[test]
fn provider_capacity_one_serializes_segments_into_deterministic_waves() {
    let candidates = vec![
        default_candidate("job-03", "S03", "voice-a"),
        default_candidate("job-01", "S01", "voice-a"),
        default_candidate("job-02", "S02", "voice-a"),
    ];
    let plan = plan_voice_burst_v1(&candidates, &[provider_snapshot(1)]).unwrap();

    assert_eq!(plan.scheduled_job_count(), 3);
    assert_eq!(plan.waves.len(), 3);
    assert!(plan.waves.iter().all(|wave| wave.assignments.len() == 1));
    assert_eq!(plan.waves[0].assignments[0].job_id, "job-01");
    assert_eq!(plan.waves[1].assignments[0].job_id, "job-02");
    assert_eq!(plan.waves[2].assignments[0].job_id, "job-03");
}

#[test]
fn unsupported_model_group_is_blocked_without_poisoning_supported_jobs() {
    let supported = default_candidate("job-ok", "S01", "voice-a");
    let unsupported = candidate(
        "job-blocked",
        "prj-a",
        "S02",
        "voice-model",
        "1.0",
        "voice-model-unsupported",
        "voice-a",
        "v1",
        12_288,
        true,
    );

    let plan = plan_voice_burst_v1(&[unsupported, supported], &[provider_snapshot(2)]).unwrap();

    assert_eq!(plan.scheduled_job_count(), 1);
    assert_eq!(plan.blocked.len(), 1);
    assert_eq!(plan.blocked[0].job_id, "job-blocked");
    assert!(plan.blocked[0]
        .decision
        .reasons
        .iter()
        .any(|reason| reason.code == GpuNotReadyReasonCodeV1::ModelGroupUnsupported));
}

#[test]
fn planner_never_pools_vram_across_two_t4_devices() {
    let too_large = candidate(
        "job-large",
        "prj-a",
        "S01",
        "voice-model",
        "1.0",
        "voice-model-a",
        "voice-a",
        "v1",
        20_000,
        true,
    );

    let plan = plan_voice_burst_v1(&[too_large], &[provider_snapshot(2)]).unwrap();

    assert_eq!(plan.scheduled_job_count(), 0);
    assert_eq!(plan.blocked.len(), 1);
    assert!(plan.blocked[0]
        .decision
        .reasons
        .iter()
        .any(|reason| reason.code == GpuNotReadyReasonCodeV1::InsufficientVram));
}

#[test]
fn non_parallelizable_affinity_group_runs_one_segment_per_wave() {
    let first = candidate(
        "job-01",
        "prj-a",
        "S01",
        "voice-model",
        "1.0",
        "voice-model-a",
        "voice-a",
        "v1",
        12_288,
        false,
    );
    let second = candidate(
        "job-02",
        "prj-a",
        "S02",
        "voice-model",
        "1.0",
        "voice-model-a",
        "voice-a",
        "v1",
        12_288,
        false,
    );

    let plan = plan_voice_burst_v1(&[second, first], &[provider_snapshot(2)]).unwrap();

    assert_eq!(plan.scheduled_job_count(), 2);
    assert_eq!(plan.waves.len(), 2);
    assert_eq!(plan.waves[0].assignments[0].job_id, "job-01");
    assert_eq!(plan.waves[1].assignments[0].job_id, "job-02");
}

#[test]
fn duplicate_job_identity_and_stale_job_hash_are_rejected() {
    let first = default_candidate("job-01", "S01", "voice-a");
    let duplicate = default_candidate("job-01", "S02", "voice-a");

    assert!(plan_voice_burst_v1(&[first.clone(), duplicate], &[provider_snapshot(2)]).is_err());

    let mut stale = first;
    stale.job.input_hash = "stale-input-hash".to_owned();
    assert!(plan_voice_burst_v1(&[stale], &[provider_snapshot(2)]).is_err());
}

#[test]
fn voice_burst_core_contract_has_no_provider_specific_implementation_fields() {
    let source = include_str!("../src/voice_burst.rs").to_lowercase();

    for forbidden in ["kaggle", "notebook", "omnivoice"] {
        assert!(
            !source.contains(forbidden),
            "voice burst core contract leaked provider-specific term {forbidden}"
        );
    }
}
