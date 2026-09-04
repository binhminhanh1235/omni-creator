use chrono::{DateTime, Utc};
use omnicreator_core::{
    evaluate_gpu_queue, normalize_segment_text_v1, CacheLookupV1, ComputeProviderCapabilitiesV1,
    ComputeProviderConnectionState, ComputeProviderSchedulingSnapshotV1,
    ComputeProviderSessionIdentityV1, ComputeProviderSessionV1, GpuNotReadyReasonCodeV1,
    GpuQueueEligibilityStatusV1, GpuReadinessFactsV1, LogicalUri, PronunciationRuleV1,
    SegmentTtsExecutionTargetV1, SegmentTtsLockStateV1, SegmentTtsPreflightIssueCodeV1,
    SegmentTtsPreflightStatusV1, SegmentTtsPreparationV1, SegmentTtsProductionInputV1, SegmentV1,
    StateStore, StepStatus, VoiceDirectionV1, VoiceIdentityV1, VoiceModelIdentityV1, Workspace,
    SEGMENT_SCHEMA, SEGMENT_SCHEMA_VERSION,
};

fn segment(text: &str) -> SegmentV1 {
    SegmentV1 {
        schema: SEGMENT_SCHEMA.to_owned(),
        schema_version: SEGMENT_SCHEMA_VERSION,
        id: "S04".to_owned(),
        order: 4,
        text: text.to_owned(),
        voice_direction: VoiceDirectionV1 {
            tone: Some("warm".to_owned()),
            pace: Some("measured".to_owned()),
            tags: vec!["soft".to_owned(), "reflective".to_owned()],
        },
    }
}

fn pronunciation_rules() -> Vec<PronunciationRuleV1> {
    vec![
        PronunciationRuleV1 {
            written: "Proverbs".to_owned(),
            pronunciation: "PRAH-verbs".to_owned(),
        },
        PronunciationRuleV1 {
            written: "S04".to_owned(),
            pronunciation: "section four".to_owned(),
        },
    ]
}

fn production_input(text: &str) -> SegmentTtsProductionInputV1 {
    SegmentTtsProductionInputV1::from_segment_v1(
        &segment(text),
        pronunciation_rules(),
        VoiceIdentityV1 {
            voice_id: "warm-narrator".to_owned(),
            voice_version: "v4".to_owned(),
        },
        VoiceModelIdentityV1 {
            model_id: "voice-model".to_owned(),
            model_version: "3.2".to_owned(),
        },
        "settings-fingerprint-v1",
    )
}

fn execution() -> SegmentTtsExecutionTargetV1 {
    SegmentTtsExecutionTargetV1 {
        plugin_id: Some("voice-provider".to_owned()),
        provider_id: Some("compute-provider".to_owned()),
        output_uri: Some(LogicalUri::parse("project://audio/S04.wav").unwrap()),
        approval_required: false,
        approval_complete: true,
        production_lock_required: true,
        gpu_execution_requested: true,
        requirements: omnicreator_core::default_segment_tts_compute_requirements_v1(
            "voice-model-3.2",
            12_288,
        ),
    }
}

fn preparation(text: &str) -> SegmentTtsPreparationV1 {
    SegmentTtsPreparationV1 {
        segment_id: "S04".to_owned(),
        production_input: production_input(text),
        locks: SegmentTtsLockStateV1 {
            normalization_locked: true,
            pronunciation_locked: true,
        },
        execution: execution(),
    }
}

fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-04T15:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn provider_snapshot() -> ComputeProviderSchedulingSnapshotV1 {
    let mut capabilities = ComputeProviderCapabilitiesV1::from_json_v1(include_str!(
        "fixtures/contracts/v1/compute-capabilities.json"
    ))
    .unwrap();
    capabilities.provider_id = "compute-provider".to_owned();
    capabilities.model_groups = vec!["voice-model-3.2".to_owned()];

    ComputeProviderSchedulingSnapshotV1 {
        state: ComputeProviderConnectionState::Ready,
        session: ComputeProviderSessionV1 {
            identity: ComputeProviderSessionIdentityV1 {
                provider_id: "compute-provider".to_owned(),
                session_id: "session-voice-p0".to_owned(),
            },
            connected_at: fixed_time(),
            last_heartbeat_at: fixed_time(),
            last_healthy_heartbeat_at: Some(fixed_time()),
            capabilities,
        },
    }
}

fn readiness(production_locked: bool) -> GpuReadinessFactsV1 {
    GpuReadinessFactsV1 {
        workflow_step_status: Some(StepStatus::Ready),
        dependencies_succeeded: true,
        production_locked,
        cache_lookup: CacheLookupV1::Miss,
    }
}

fn reason_codes(
    decision: &omnicreator_core::GpuQueueEligibilityV1,
) -> Vec<GpuNotReadyReasonCodeV1> {
    decision.reasons.iter().map(|reason| reason.code).collect()
}

#[test]
fn whitespace_normalization_is_conservative_and_deterministic() {
    assert_eq!(
        normalize_segment_text_v1("  Love\n\n stays   truthful.  "),
        "Love stays truthful."
    );
    assert_eq!(
        normalize_segment_text_v1("Love stays truthful."),
        "Love stays truthful."
    );
}

#[test]
fn input_hash_is_stable_across_equivalent_whitespace_and_rule_order() {
    let first = production_input("  Love\n stays   truthful. ");
    let mut second = production_input("Love stays truthful.");
    second.pronunciation_rules.reverse();

    assert_eq!(
        first.input_hash_v1().unwrap(),
        second.input_hash_v1().unwrap()
    );
}

#[test]
fn meaningful_source_edit_requires_renormalization_and_changes_hash() {
    let locked = production_input("Love stays truthful.");
    let old_hash = locked.input_hash_v1().unwrap();

    let mut stale = locked.clone();
    stale.source_text = "Love stays truthful even when it costs something.".to_owned();

    assert!(stale.input_hash_v1().is_err());
    let stale_preflight = omnicreator_core::evaluate_segment_tts_preflight_v1(
        &stale,
        &SegmentTtsLockStateV1 {
            normalization_locked: true,
            pronunciation_locked: true,
        },
        &execution(),
    );
    assert_eq!(stale_preflight.status, SegmentTtsPreflightStatusV1::Blocked);
    assert!(stale_preflight.has(SegmentTtsPreflightIssueCodeV1::NormalizationStale));

    let refreshed = production_input("Love stays truthful even when it costs something.");
    assert_ne!(old_hash, refreshed.input_hash_v1().unwrap());
}

#[test]
fn pronunciation_voice_model_direction_and_settings_all_participate_in_hash() {
    let baseline = production_input("Love stays truthful.");
    let baseline_hash = baseline.input_hash_v1().unwrap();

    let mut pronunciation = baseline.clone();
    pronunciation.pronunciation_rules[0].pronunciation = "PROH-verbs".to_owned();
    assert_ne!(baseline_hash, pronunciation.input_hash_v1().unwrap());

    let mut voice = baseline.clone();
    voice.voice.voice_version = "v5".to_owned();
    assert_ne!(baseline_hash, voice.input_hash_v1().unwrap());

    let mut model = baseline.clone();
    model.model.model_version = "3.3".to_owned();
    assert_ne!(baseline_hash, model.input_hash_v1().unwrap());

    let mut direction = baseline.clone();
    direction.voice_direction.pace = Some("slow".to_owned());
    assert_ne!(baseline_hash, direction.input_hash_v1().unwrap());

    let mut settings = baseline.clone();
    settings.settings_fingerprint = "settings-fingerprint-v2".to_owned();
    assert_ne!(baseline_hash, settings.input_hash_v1().unwrap());
}

#[test]
fn conflicting_pronunciation_and_unlocked_inputs_are_typed_preflight_blockers() {
    let mut prep = preparation("Love stays truthful.");
    prep.locks.normalization_locked = false;
    prep.locks.pronunciation_locked = false;
    prep.production_input
        .pronunciation_rules
        .push(PronunciationRuleV1 {
            written: "proverbs".to_owned(),
            pronunciation: "PROH-verbs".to_owned(),
        });

    let preflight = prep.preflight_v1().unwrap();
    assert_eq!(preflight.status, SegmentTtsPreflightStatusV1::Blocked);
    assert!(preflight.has(SegmentTtsPreflightIssueCodeV1::NormalizationUnlocked));
    assert!(preflight.has(SegmentTtsPreflightIssueCodeV1::PronunciationUnlocked));
    assert!(preflight.has(SegmentTtsPreflightIssueCodeV1::PronunciationConflict));
    assert!(preflight
        .issues
        .iter()
        .all(|issue| !issue.message.trim().is_empty()));
}

#[test]
fn missing_voice_model_settings_plugin_provider_and_output_are_reported_together() {
    let mut prep = preparation("Love stays truthful.");
    prep.production_input.voice.voice_id.clear();
    prep.production_input.voice.voice_version.clear();
    prep.production_input.model.model_id.clear();
    prep.production_input.model.model_version.clear();
    prep.production_input.settings_fingerprint.clear();
    prep.execution.plugin_id = None;
    prep.execution.provider_id = None;
    prep.execution.output_uri = None;

    let preflight = prep.preflight_v1().unwrap();
    for code in [
        SegmentTtsPreflightIssueCodeV1::VoiceMissing,
        SegmentTtsPreflightIssueCodeV1::VoiceVersionMissing,
        SegmentTtsPreflightIssueCodeV1::ModelMissing,
        SegmentTtsPreflightIssueCodeV1::ModelVersionMissing,
        SegmentTtsPreflightIssueCodeV1::SettingsMissing,
        SegmentTtsPreflightIssueCodeV1::PluginMissing,
        SegmentTtsPreflightIssueCodeV1::ProviderMissing,
        SegmentTtsPreflightIssueCodeV1::OutputMissing,
    ] {
        assert!(preflight.has(code), "missing preflight issue {code:?}");
    }
}

#[test]
fn locked_segment_maps_into_existing_gpu_ready_contract() {
    let prep = preparation("Love stays truthful.");
    let preflight = prep.preflight_v1().unwrap();
    assert!(preflight.is_ready());

    let input_hash = prep.input_hash_v1().unwrap();
    let gpu_prep = prep.to_gpu_job_preparation_v1("job-S04").unwrap();
    let job = omnicreator_core::Job {
        job_id: "job-S04".to_owned(),
        project_id: "prj-voice".to_owned(),
        step: "tts".to_owned(),
        unit: "S04".to_owned(),
        status: StepStatus::Ready,
        input_hash,
        selected_attempt: None,
        selected_artifact: None,
    };

    let decision = evaluate_gpu_queue(
        &job,
        &readiness(true),
        &gpu_prep,
        &[provider_snapshot()],
        &[],
    )
    .unwrap();

    assert_eq!(decision.status, GpuQueueEligibilityStatusV1::GpuReady);
    assert!(decision.reasons.is_empty());
    assert_eq!(decision.selection.unwrap().device_id, "gpu0");
}

#[test]
fn blocked_tts_preflight_becomes_scheduler_preflight_pending() {
    let mut prep = preparation("Love stays truthful.");
    prep.locks.pronunciation_locked = false;
    let gpu_prep = prep.to_gpu_job_preparation_v1("job-S04").unwrap();
    assert!(!gpu_prep.preflight_complete);
    assert!(!gpu_prep.input_immutable);

    let job = omnicreator_core::Job {
        job_id: "job-S04".to_owned(),
        project_id: "prj-voice".to_owned(),
        step: "tts".to_owned(),
        unit: "S04".to_owned(),
        status: StepStatus::Ready,
        input_hash: prep.input_hash_v1().unwrap(),
        selected_attempt: None,
        selected_artifact: None,
    };
    let decision = evaluate_gpu_queue(
        &job,
        &readiness(true),
        &gpu_prep,
        &[provider_snapshot()],
        &[],
    )
    .unwrap();
    let codes = reason_codes(&decision);

    assert!(codes.contains(&GpuNotReadyReasonCodeV1::InputNotImmutable));
    assert!(codes.contains(&GpuNotReadyReasonCodeV1::PreflightPending));
    assert!(decision.selection.is_none());
}

#[test]
fn project_production_lock_remains_a_canonical_scheduler_gate() {
    let prep = preparation("Love stays truthful.");
    let gpu_prep = prep.to_gpu_job_preparation_v1("job-S04").unwrap();
    let job = omnicreator_core::Job {
        job_id: "job-S04".to_owned(),
        project_id: "prj-voice".to_owned(),
        step: "tts".to_owned(),
        unit: "S04".to_owned(),
        status: StepStatus::Ready,
        input_hash: prep.input_hash_v1().unwrap(),
        selected_attempt: None,
        selected_artifact: None,
    };

    let decision = evaluate_gpu_queue(
        &job,
        &readiness(false),
        &gpu_prep,
        &[provider_snapshot()],
        &[],
    )
    .unwrap();

    assert!(reason_codes(&decision).contains(&GpuNotReadyReasonCodeV1::ProductionLockMissing));
    assert!(decision.selection.is_none());
}

#[test]
fn source_edit_invalidates_only_affected_segment_and_downstream_steps() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Voice invalidation").unwrap();

    let old_hash = preparation("Love stays truthful.").input_hash_v1().unwrap();
    let new_hash = preparation("Love stays truthful even under pressure.")
        .input_hash_v1()
        .unwrap();
    assert_ne!(old_hash, new_hash);

    let voice_prep = state
        .create_step(
            &project.id,
            "voice-prep",
            "S04",
            StepStatus::Succeeded,
            Some(&old_hash),
        )
        .unwrap();
    let tts = state
        .create_step(
            &project.id,
            "tts",
            "S04",
            StepStatus::Succeeded,
            Some(&old_hash),
        )
        .unwrap();
    let timing = state
        .create_step(
            &project.id,
            "timing",
            "S04",
            StepStatus::Succeeded,
            Some("timing-old"),
        )
        .unwrap();
    let unrelated = state
        .create_step(
            &project.id,
            "tts",
            "S05",
            StepStatus::Succeeded,
            Some("other-segment"),
        )
        .unwrap();

    state
        .add_dependency(&voice_prep.step_id, &tts.step_id)
        .unwrap();
    state.add_dependency(&tts.step_id, &timing.step_id).unwrap();

    let impact = state
        .invalidate_from(&voice_prep.step_id, Some(&new_hash))
        .unwrap();

    assert_eq!(impact.len(), 3);
    assert_eq!(
        state.get_step(&voice_prep.step_id).unwrap().status,
        StepStatus::Stale
    );
    assert_eq!(
        state.get_step(&tts.step_id).unwrap().status,
        StepStatus::Stale
    );
    assert_eq!(
        state.get_step(&timing.step_id).unwrap().status,
        StepStatus::Stale
    );
    assert_eq!(
        state.get_step(&unrelated.step_id).unwrap().status,
        StepStatus::Succeeded
    );
    assert_eq!(
        state
            .get_step(&voice_prep.step_id)
            .unwrap()
            .input_hash
            .as_deref(),
        Some(new_hash.as_str())
    );
}

#[test]
fn voice_production_core_contract_contains_no_provider_specific_implementation_fields() {
    let source = include_str!("../src/voice_production.rs").to_lowercase();

    for forbidden in ["kaggle", "notebook", "omnivoice"] {
        assert!(
            !source.contains(forbidden),
            "voice core contract leaked provider-specific term {forbidden}"
        );
    }
}
