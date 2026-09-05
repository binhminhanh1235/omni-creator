use std::{fs, path::Path};

use omnicreator_core::{
    dispatch_remote_voice_take_v1, sync_remote_voice_artifact_bundle_v1, ArtifactStore,
    ComputeDeviceSelectionV1, ComputeJobDispatchAckV1, ComputeJobDispatchV1,
    ComputeProviderExecution, ComputeRemoteArtifactV1, ComputeRemoteJournalEntryV1,
    ComputeRemoteJournalEventV1, ComputeRequirements, GpuJobPreparationV1,
    GpuQueueEligibilityStatusV1, GpuQueueEligibilityV1, LogicalUri, RemoteArtifactSyncOutcomeV1,
    RemoteComputeJobSpecV1, RemoteVoiceBundleSyncOutcomeV1, ResourceRequirement,
    SegmentTtsProductionInputV1, SegmentV1, StateStore, StepStatus, VoiceDirectionV1,
    VoiceIdentityV1, VoiceModelIdentityV1, Workspace, REMOTE_JOURNAL_SCHEMA_V1, SEGMENT_SCHEMA,
    SEGMENT_SCHEMA_VERSION,
};
use sha2::{Digest, Sha256};

#[derive(Default)]
struct FakeExecutor {
    dispatched: Vec<ComputeJobDispatchV1>,
    transfer_bytes: Vec<u8>,
    transfer_calls: usize,
}

impl ComputeProviderExecution for FakeExecutor {
    fn dispatch_job(
        &mut self,
        dispatch: &ComputeJobDispatchV1,
    ) -> omnicreator_core::Result<ComputeJobDispatchAckV1> {
        self.dispatched.push(dispatch.clone());
        Ok(ComputeJobDispatchAckV1 {
            job_id: dispatch.job_id.clone(),
            attempt_id: dispatch.attempt_id.clone(),
            remote_job_ref: format!("remote-{}", dispatch.attempt_id),
        })
    }

    fn read_journal(
        &mut self,
        _provider_id: &str,
        _session_id: &str,
        _after_sequence: Option<u64>,
    ) -> omnicreator_core::Result<Vec<ComputeRemoteJournalEntryV1>> {
        Ok(Vec::new())
    }

    fn transfer_artifact(
        &mut self,
        _provider_id: &str,
        _session_id: &str,
        _transfer_ref: &str,
        destination: &Path,
    ) -> omnicreator_core::Result<()> {
        self.transfer_calls += 1;
        let bytes = if _transfer_ref.starts_with("timing-") {
            timing_bytes()
        } else {
            self.transfer_bytes.clone()
        };
        fs::write(destination, bytes)?;
        Ok(())
    }
}

fn segment_input(voice_version: &str) -> SegmentTtsProductionInputV1 {
    let segment = SegmentV1 {
        schema: SEGMENT_SCHEMA.to_owned(),
        schema_version: SEGMENT_SCHEMA_VERSION,
        id: "S01".to_owned(),
        order: 1,
        text: "Love stays truthful.".to_owned(),
        voice_direction: VoiceDirectionV1 {
            tone: Some("warm".to_owned()),
            pace: Some("measured".to_owned()),
            tags: vec!["reflective".to_owned()],
        },
    };
    SegmentTtsProductionInputV1::from_segment_v1(
        &segment,
        Vec::new(),
        VoiceIdentityV1 {
            voice_id: "warm-narrator".to_owned(),
            voice_version: voice_version.to_owned(),
        },
        VoiceModelIdentityV1 {
            model_id: "voice-model".to_owned(),
            model_version: "3.2".to_owned(),
        },
        "settings-v1",
    )
}

fn selection() -> ComputeDeviceSelectionV1 {
    ComputeDeviceSelectionV1 {
        provider_id: "compute-provider".to_owned(),
        session_id: "session-takes".to_owned(),
        device_id: "gpu0".to_owned(),
        parallelizable: true,
        parallelism_group: "voice-model-3.2".to_owned(),
    }
}

fn gpu_ready(job_id: &str) -> GpuQueueEligibilityV1 {
    GpuQueueEligibilityV1 {
        job_id: job_id.to_owned(),
        status: GpuQueueEligibilityStatusV1::GpuReady,
        reasons: Vec::new(),
        selection: Some(selection()),
    }
}

fn preparation(job_id: &str) -> GpuJobPreparationV1 {
    GpuJobPreparationV1 {
        job_id: job_id.to_owned(),
        input_resolved: true,
        input_immutable: true,
        plugin_id: Some("voice-provider".to_owned()),
        provider_id: Some("compute-provider".to_owned()),
        model_id: Some("voice-model".to_owned()),
        model_version: Some("3.2".to_owned()),
        settings_fingerprint: Some("settings-v1".to_owned()),
        output_uri: Some(LogicalUri::parse("project://audio/S01.wav").unwrap()),
        approval_required: false,
        approval_complete: true,
        production_lock_required: false,
        preflight_required: true,
        preflight_complete: true,
        gpu_execution_requested: true,
        requirements: ComputeRequirements {
            gpu: ResourceRequirement::Required,
            min_vram_mb: Some(12_288),
            model_group: Some("voice-model-3.2".to_owned()),
            parallelizable: true,
            cost_metric: Some("seconds".to_owned()),
        },
    }
}

fn spec(job_id: &str) -> RemoteComputeJobSpecV1 {
    RemoteComputeJobSpecV1 {
        job_id: job_id.to_owned(),
        operation: "tts.generate".to_owned(),
        plugin_payload: serde_json::json!({
            "segment_id": "S01",
            "voice_id": "warm-narrator"
        }),
    }
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn timing_bytes() -> Vec<u8> {
    serde_json::to_vec_pretty(&serde_json::json!({
        "schema": "omnicreator.voice-timing",
        "version": 1,
        "segment_id": "S01",
        "duration_ms": 1500,
        "cues": [
            {"index": 0, "text": "Love stays truthful.", "start_ms": 0, "end_ms": 1500}
        ]
    }))
    .unwrap()
}

fn artifact_entry(
    started: &omnicreator_core::RemoteDispatchStartedV1,
    input_hash: &str,
    bytes: &[u8],
) -> ComputeRemoteJournalEntryV1 {
    let timing = timing_bytes();
    ComputeRemoteJournalEntryV1 {
        schema: REMOTE_JOURNAL_SCHEMA_V1.to_owned(),
        version: 1,
        sequence: 3,
        provider_id: started.dispatch.provider_id.clone(),
        session_id: started.dispatch.session_id.clone(),
        job_id: started.dispatch.job_id.clone(),
        attempt_id: started.attempt_id.clone(),
        input_hash: input_hash.to_owned(),
        event: ComputeRemoteJournalEventV1::ArtifactBundleReady {
            artifacts: vec![
                ComputeRemoteArtifactV1 {
                    artifact_type: "audio".to_owned(),
                    output_uri: started.dispatch.output_uri.clone(),
                    sha256: sha256(bytes),
                    size_bytes: bytes.len() as u64,
                    transfer_ref: format!("audio-{}", started.attempt_id),
                },
                ComputeRemoteArtifactV1 {
                    artifact_type: "voice_timing".to_owned(),
                    output_uri: omnicreator_core::voice_timing_output_uri_v1(
                        &started.dispatch.output_uri,
                    )
                    .unwrap(),
                    sha256: sha256(&timing),
                    size_bytes: timing.len() as u64,
                    transfer_ref: format!("timing-{}", started.attempt_id),
                },
            ],
        },
    }
}

fn sync_take(
    state: &mut StateStore,
    artifacts: &ArtifactStore,
    executor: &mut FakeExecutor,
    started: &omnicreator_core::RemoteDispatchStartedV1,
    input_hash: &str,
    bytes: &[u8],
    staging: &Path,
) -> RemoteArtifactSyncOutcomeV1 {
    executor.transfer_bytes = bytes.to_vec();
    match sync_remote_voice_artifact_bundle_v1(
        state,
        artifacts,
        executor,
        &artifact_entry(started, input_hash, bytes),
        staging,
        serde_json::json!({"kind":"voice_take"}),
    )
    .unwrap()
    {
        RemoteVoiceBundleSyncOutcomeV1::Committed(bundle) => {
            RemoteArtifactSyncOutcomeV1::Committed(bundle.audio)
        }
        RemoteVoiceBundleSyncOutcomeV1::AlreadyCommitted(bundle) => {
            RemoteArtifactSyncOutcomeV1::AlreadyCommitted(bundle.audio)
        }
    }
}

#[test]
fn successful_retakes_preserve_history_and_do_not_auto_reselect() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Voice Takes").unwrap();
    let input_hash = segment_input("v4").input_hash_v1().unwrap();
    let job = state
        .create_job(&project.id, "tts", "S01", &input_hash)
        .unwrap();
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let mut executor = FakeExecutor::default();

    let first = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    assert_eq!(
        first.dispatch.output_uri,
        LogicalUri::parse("project://audio/takes/S01/take-0001.wav").unwrap()
    );
    let first_bytes = b"voice take one";
    let first_artifact = match sync_take(
        &mut state,
        &artifacts,
        &mut executor,
        &first,
        &input_hash,
        first_bytes,
        &temp.path().join("staging-1"),
    ) {
        RemoteArtifactSyncOutcomeV1::Committed(artifact) => artifact,
        other => panic!("expected committed first take, got {other:?}"),
    };

    let after_first = state.get_job(&job.job_id).unwrap();
    assert_eq!(after_first.status, StepStatus::Succeeded);
    assert_eq!(
        after_first.selected_attempt.as_deref(),
        Some(first.attempt_id.as_str())
    );
    assert_eq!(
        after_first.selected_artifact.as_deref(),
        Some(first_artifact.artifact_id.as_str())
    );

    let retake_requested = state.request_voice_retake_v1(&job.job_id).unwrap();
    assert_eq!(retake_requested.status, StepStatus::Ready);
    assert_eq!(
        retake_requested.selected_attempt.as_deref(),
        Some(first.attempt_id.as_str())
    );
    assert_eq!(
        retake_requested.selected_artifact.as_deref(),
        Some(first_artifact.artifact_id.as_str())
    );

    let second = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    assert_eq!(
        second.dispatch.output_uri,
        LogicalUri::parse("project://audio/takes/S01/take-0002.wav").unwrap()
    );
    let second_bytes = b"voice take two";
    let second_artifact = match sync_take(
        &mut state,
        &artifacts,
        &mut executor,
        &second,
        &input_hash,
        second_bytes,
        &temp.path().join("staging-2"),
    ) {
        RemoteArtifactSyncOutcomeV1::Committed(artifact) => artifact,
        other => panic!("expected committed second take, got {other:?}"),
    };

    let after_second = state.get_job(&job.job_id).unwrap();
    assert_eq!(after_second.status, StepStatus::Succeeded);
    assert_eq!(
        after_second.selected_attempt.as_deref(),
        Some(first.attempt_id.as_str())
    );
    assert_eq!(
        after_second.selected_artifact.as_deref(),
        Some(first_artifact.artifact_id.as_str())
    );

    let takes = state.list_voice_takes_v1(&job.job_id).unwrap();
    assert_eq!(takes.len(), 2);
    assert_eq!(takes[0].take_index, 1);
    assert_eq!(takes[1].take_index, 2);
    assert!(takes[0].selected);
    assert!(!takes[1].selected);
    assert_eq!(
        takes[1].artifact.as_ref().unwrap().artifact_id,
        second_artifact.artifact_id
    );

    for relative in [
        "audio/takes/S01/take-0001.wav",
        "audio/takes/S01/take-0002.wav",
    ] {
        assert!(workspace
            .data_root()
            .join("projects")
            .join(&project.id)
            .join(relative)
            .exists());
    }

    state
        .select_voice_take_v1(&job.job_id, &second.attempt_id)
        .unwrap();
    let selected_second = state.get_job(&job.job_id).unwrap();
    assert_eq!(
        selected_second.selected_attempt.as_deref(),
        Some(second.attempt_id.as_str())
    );

    state
        .select_voice_take_v1(&job.job_id, &first.attempt_id)
        .unwrap();
    let reselected_first = state.get_job(&job.job_id).unwrap();
    assert_eq!(
        reselected_first.selected_attempt.as_deref(),
        Some(first.attempt_id.as_str())
    );
}

#[test]
fn duplicate_delivery_uses_attempt_artifact_not_selected_take() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Duplicate Voice Take").unwrap();
    let input_hash = segment_input("v4").input_hash_v1().unwrap();
    let job = state
        .create_job(&project.id, "tts", "S01", &input_hash)
        .unwrap();
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let mut executor = FakeExecutor::default();

    let first = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    sync_take(
        &mut state,
        &artifacts,
        &mut executor,
        &first,
        &input_hash,
        b"take one",
        &temp.path().join("staging-first"),
    );

    state.request_voice_retake_v1(&job.job_id).unwrap();
    let second = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    let bytes = b"take two";
    sync_take(
        &mut state,
        &artifacts,
        &mut executor,
        &second,
        &input_hash,
        bytes,
        &temp.path().join("staging-second"),
    );
    let transfers_before_duplicate = executor.transfer_calls;

    let duplicate = sync_take(
        &mut state,
        &artifacts,
        &mut executor,
        &second,
        &input_hash,
        bytes,
        &temp.path().join("staging-duplicate"),
    );

    assert!(matches!(
        duplicate,
        RemoteArtifactSyncOutcomeV1::AlreadyCommitted(_)
    ));
    assert_eq!(executor.transfer_calls, transfers_before_duplicate);
    assert_eq!(
        state
            .get_job(&job.job_id)
            .unwrap()
            .selected_attempt
            .as_deref(),
        Some(first.attempt_id.as_str())
    );
}

#[test]
fn failed_retake_keeps_job_identity_selected_take_and_retry_request() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Retry Voice Take").unwrap();
    let input_hash = segment_input("v4").input_hash_v1().unwrap();
    let job = state
        .create_job(&project.id, "tts", "S01", &input_hash)
        .unwrap();
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let mut executor = FakeExecutor::default();

    let first = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    sync_take(
        &mut state,
        &artifacts,
        &mut executor,
        &first,
        &input_hash,
        b"selected take",
        &temp.path().join("staging-first"),
    );

    state.request_voice_retake_v1(&job.job_id).unwrap();
    let failed = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    state
        .finish_attempt_failure(&failed.attempt_id, "NETWORK_TIMEOUT")
        .unwrap();

    let after_failure = state.get_job(&job.job_id).unwrap();
    assert_eq!(after_failure.job_id, job.job_id);
    assert_eq!(after_failure.status, StepStatus::Retryable);
    assert_eq!(
        after_failure.selected_attempt.as_deref(),
        Some(first.attempt_id.as_str())
    );
    assert!(state.has_active_voice_retake_v1(&job.job_id).unwrap());

    let retry = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    assert_eq!(retry.dispatch.job_id, job.job_id);
    assert_ne!(retry.attempt_id, failed.attempt_id);
    assert_eq!(
        retry.dispatch.output_uri,
        LogicalUri::parse("project://audio/takes/S01/take-0003.wav").unwrap()
    );

    let takes = state.list_voice_takes_v1(&job.job_id).unwrap();
    assert_eq!(takes.len(), 3);
    assert_eq!(takes[1].attempt.status, StepStatus::Retryable);
    assert_eq!(takes[2].attempt.status, StepStatus::Running);
    assert!(takes[0].selected);
}

#[test]
fn verified_voice_cache_is_keyed_by_immutable_segment_voice_model_hash() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Voice Cache").unwrap();
    let input_hash = segment_input("v4").input_hash_v1().unwrap();
    let changed_voice_hash = segment_input("v5").input_hash_v1().unwrap();
    assert_ne!(input_hash, changed_voice_hash);

    let job = state
        .create_job(&project.id, "tts", "S01", &input_hash)
        .unwrap();
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let mut executor = FakeExecutor::default();
    let first = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    sync_take(
        &mut state,
        &artifacts,
        &mut executor,
        &first,
        &input_hash,
        b"cacheable voice take",
        &temp.path().join("staging-cache"),
    );

    let hit = artifacts
        .lookup_verified_voice_take_cache_v1(&state, &input_hash)
        .unwrap()
        .expect("same immutable voice input should hit cache");
    assert_eq!(hit.attempt.attempt_id, first.attempt_id);
    assert!(hit.artifact.is_some());

    assert!(artifacts
        .lookup_verified_voice_take_cache_v1(&state, &changed_voice_hash)
        .unwrap()
        .is_none());

    state.request_voice_retake_v1(&job.job_id).unwrap();
    let hit_while_retake_pending = artifacts
        .lookup_verified_voice_take_cache_v1(&state, &input_hash)
        .unwrap()
        .expect("verified prior take remains cache-visible during retake");
    assert_eq!(
        hit_while_retake_pending.attempt.attempt_id,
        first.attempt_id
    );
}

#[test]
fn voice_retake_readiness_bypasses_cache_and_completed_step_without_mutating_it() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Retake Readiness").unwrap();
    let input_hash = segment_input("v4").input_hash_v1().unwrap();
    let step = state
        .create_step(
            &project.id,
            "tts",
            "S01",
            StepStatus::Succeeded,
            Some(&input_hash),
        )
        .unwrap();
    let job = state
        .create_job(&project.id, "tts", "S01", &input_hash)
        .unwrap();
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let mut executor = FakeExecutor::default();

    let first = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    sync_take(
        &mut state,
        &artifacts,
        &mut executor,
        &first,
        &input_hash,
        b"baseline",
        &temp.path().join("staging-readiness"),
    );

    state.request_voice_retake_v1(&job.job_id).unwrap();
    let retake_job = state.get_job(&job.job_id).unwrap();
    let facts = state.voice_gpu_readiness_facts_v1(&retake_job).unwrap();

    assert_eq!(facts.workflow_step_status, Some(StepStatus::Ready));
    assert!(matches!(
        facts.cache_lookup,
        omnicreator_core::CacheLookupV1::Miss
    ));
    assert_eq!(
        state.get_step(&step.step_id).unwrap().status,
        StepStatus::Succeeded
    );
}

#[test]
fn take_output_uri_is_portable_and_attempt_history_contains_no_absolute_paths() {
    let base = LogicalUri::parse("project://audio/S01.wav").unwrap();
    assert_eq!(
        omnicreator_core::voice_take_output_uri_v1(&base, 12).unwrap(),
        LogicalUri::parse("project://audio/takes/S01/take-0012.wav").unwrap()
    );

    let source = include_str!("../src/voice_takes.rs").to_lowercase();
    for forbidden in ["kaggle", "notebook", "c:\\", "/home/", "/users/"] {
        assert!(
            !source.contains(forbidden),
            "voice take contract leaked provider/machine-specific term {forbidden}"
        );
    }
}
