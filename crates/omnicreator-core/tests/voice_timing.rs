use std::{collections::BTreeMap, fs, path::Path};

use omnicreator_core::{
    dispatch_remote_voice_take_v1, reconcile_remote_session_v1, sync_remote_artifact,
    sync_remote_voice_artifact_bundle_v1, voice_timing_output_uri_v1, ArtifactStore,
    ComputeDeviceSelectionV1, ComputeJobDispatchAckV1, ComputeJobDispatchV1,
    ComputeProviderConnectionState, ComputeProviderExecution, ComputeRemoteArtifactV1,
    ComputeRemoteJournalEntryV1, ComputeRemoteJournalEventV1, ComputeRequirements, Error,
    GpuJobPreparationV1, GpuQueueEligibilityStatusV1, GpuQueueEligibilityV1, LogicalUri,
    RemoteComputeJobSpecV1, RemoteVoiceBundleSyncOutcomeV1,
    ResourceRequirement, StateStore, StepStatus, VoiceTimingCueV1, VoiceTimingV1, Workspace,
    REMOTE_JOURNAL_SCHEMA_V1, VOICE_TIMING_SCHEMA_V1,
};
use sha2::{Digest, Sha256};

#[derive(Default)]
struct FakeExecutor {
    dispatched: Vec<ComputeJobDispatchV1>,
    journal: Vec<ComputeRemoteJournalEntryV1>,
    transfers: BTreeMap<String, Vec<u8>>,
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
        provider_id: &str,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> omnicreator_core::Result<Vec<ComputeRemoteJournalEntryV1>> {
        Ok(self
            .journal
            .iter()
            .filter(|entry| {
                entry.provider_id == provider_id
                    && entry.session_id == session_id
                    && match after_sequence {
                        Some(sequence) => entry.sequence > sequence,
                        None => true,
                    }
            })
            .cloned()
            .collect())
    }

    fn transfer_artifact(
        &mut self,
        _provider_id: &str,
        _session_id: &str,
        transfer_ref: &str,
        destination: &Path,
    ) -> omnicreator_core::Result<()> {
        self.transfer_calls += 1;
        let bytes = self.transfers.get(transfer_ref).ok_or_else(|| {
            Error::InvalidContract(format!("missing fixture transfer {transfer_ref}"))
        })?;
        fs::write(destination, bytes)?;
        Ok(())
    }
}

fn selection() -> ComputeDeviceSelectionV1 {
    ComputeDeviceSelectionV1 {
        provider_id: "compute-provider".to_owned(),
        session_id: "session-timing".to_owned(),
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

fn timing_contract() -> VoiceTimingV1 {
    VoiceTimingV1 {
        schema: VOICE_TIMING_SCHEMA_V1.to_owned(),
        version: 1,
        segment_id: "S01".to_owned(),
        duration_ms: 2500,
        cues: vec![
            VoiceTimingCueV1 {
                index: 0,
                text: "Love stays".to_owned(),
                start_ms: 0,
                end_ms: 1000,
            },
            VoiceTimingCueV1 {
                index: 1,
                text: "truthful.".to_owned(),
                start_ms: 1100,
                end_ms: 2500,
            },
        ],
    }
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn bundle_entry(
    started: &omnicreator_core::RemoteDispatchStartedV1,
    input_hash: &str,
    audio: &[u8],
    timing: &[u8],
) -> ComputeRemoteJournalEntryV1 {
    let timing_uri = voice_timing_output_uri_v1(&started.dispatch.output_uri).unwrap();
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
                    sha256: sha256(audio),
                    size_bytes: audio.len() as u64,
                    transfer_ref: format!("audio-{}", started.attempt_id),
                },
                ComputeRemoteArtifactV1 {
                    artifact_type: "voice_timing".to_owned(),
                    output_uri: timing_uri,
                    sha256: sha256(timing),
                    size_bytes: timing.len() as u64,
                    transfer_ref: format!("timing-{}", started.attempt_id),
                },
            ],
        },
    }
}

fn register_transfers(
    executor: &mut FakeExecutor,
    started: &omnicreator_core::RemoteDispatchStartedV1,
    audio: &[u8],
    timing: &[u8],
) {
    executor
        .transfers
        .insert(format!("audio-{}", started.attempt_id), audio.to_vec());
    executor
        .transfers
        .insert(format!("timing-{}", started.attempt_id), timing.to_vec());
}

fn create_voice_job(
    state: &StateStore,
    project_id: &str,
    input_hash: &str,
) -> omnicreator_core::Job {
    state
        .create_job(project_id, "tts", "S01", input_hash)
        .unwrap()
}

#[test]
fn timing_contract_is_deterministic_and_caption_ready() {
    let timing = timing_contract();
    timing.validate_v1().unwrap();

    let bytes = timing.to_json_bytes_v1().unwrap();
    assert_eq!(VoiceTimingV1::from_json_bytes_v1(&bytes).unwrap(), timing);

    let captions = timing.caption_cues_v1().unwrap();
    assert_eq!(captions.len(), 2);
    assert_eq!(captions[0].start_seconds, 0.0);
    assert_eq!(captions[0].end_seconds, 1.0);
    assert_eq!(captions[1].start_seconds, 1.1);
    assert_eq!(captions[1].end_seconds, 2.5);
    assert_eq!(timing.duration_seconds_v1().unwrap(), 2.5);

    let mut overlap = timing.clone();
    overlap.cues[1].start_ms = 900;
    assert!(overlap.validate_v1().is_err());

    let mut out_of_range = timing.clone();
    out_of_range.cues[1].end_ms = 2600;
    assert!(out_of_range.validate_v1().is_err());

    let mut wrong_index = timing;
    wrong_index.cues[1].index = 7;
    assert!(wrong_index.validate_v1().is_err());
}

#[test]
fn voice_dispatch_requests_portable_timing_sidecar() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Timing Dispatch").unwrap();
    let job = create_voice_job(&state, &project.id, "input-hash-timing");
    let mut executor = FakeExecutor::default();

    let started = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();

    assert_eq!(
        started.dispatch.output_uri,
        LogicalUri::parse("project://audio/takes/S01/take-0001.wav").unwrap()
    );
    let timing = started
        .dispatch
        .plugin_payload
        .get("timing")
        .expect("timing dispatch contract");
    assert_eq!(timing["schema"], VOICE_TIMING_SCHEMA_V1);
    assert_eq!(timing["version"], 1);
    assert_eq!(
        timing["output_uri"],
        "project://audio/takes/S01/take-0001.timing.json"
    );
}

#[test]
fn audio_and_timing_commit_atomically_only_after_both_verify() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Timing Bundle").unwrap();
    let job = create_voice_job(&state, &project.id, "input-hash-timing");
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let mut executor = FakeExecutor::default();

    let started = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();

    let audio = b"verified audio bytes";
    let timing = timing_contract().to_json_bytes_v1().unwrap();
    register_transfers(&mut executor, &started, audio, &timing);
    let entry = bundle_entry(&started, &job.input_hash, audio, &timing);

    let outcome = sync_remote_voice_artifact_bundle_v1(
        &mut state,
        &artifacts,
        &mut executor,
        &entry,
        temp.path().join("staging"),
        serde_json::json!({"source":"fixture"}),
    )
    .unwrap();
    let bundle = match outcome {
        RemoteVoiceBundleSyncOutcomeV1::Committed(bundle) => bundle,
        other => panic!("expected committed bundle, got {other:?}"),
    };

    assert_eq!(
        state.get_job(&job.job_id).unwrap().status,
        StepStatus::Succeeded
    );
    assert_eq!(
        state.get_attempt(&started.attempt_id).unwrap().status,
        StepStatus::Succeeded
    );
    assert!(artifacts.verify_artifact(&bundle.audio).unwrap());
    assert!(artifacts.verify_artifact(&bundle.timing).unwrap());

    let take = state
        .get_voice_take_v1(&started.attempt_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        take.artifact.as_ref().unwrap().artifact_id,
        bundle.audio.artifact_id
    );
    assert_eq!(
        take.timing_artifact.as_ref().unwrap().artifact_id,
        bundle.timing.artifact_id
    );
    assert_eq!(
        state
            .selected_voice_timing_artifact_v1(&job.job_id)
            .unwrap()
            .unwrap()
            .artifact_id,
        bundle.timing.artifact_id
    );

    let loaded = artifacts
        .load_voice_timing_v1(&state, &started.attempt_id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded, timing_contract());
    assert_eq!(loaded.caption_cues_v1().unwrap()[1].start_seconds, 1.1);
}

#[test]
fn invalid_timing_content_cannot_mark_audio_successful() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Invalid Timing").unwrap();
    let job = create_voice_job(&state, &project.id, "input-hash-timing");
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let mut executor = FakeExecutor::default();

    let started = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();

    let audio = b"audio must not commit alone";
    let invalid_timing = br#"{
        "schema":"omnicreator.voice-timing",
        "version":1,
        "segment_id":"S01",
        "duration_ms":1000,
        "cues":[{"index":0,"text":"bad","start_ms":900,"end_ms":800}]
    }"#;
    register_transfers(&mut executor, &started, audio, invalid_timing);
    let entry = bundle_entry(&started, &job.input_hash, audio, invalid_timing);

    assert!(sync_remote_voice_artifact_bundle_v1(
        &mut state,
        &artifacts,
        &mut executor,
        &entry,
        temp.path().join("staging-invalid"),
        serde_json::json!({}),
    )
    .is_err());

    assert_eq!(
        state.get_job(&job.job_id).unwrap().status,
        StepStatus::Running
    );
    assert_eq!(
        state.get_attempt(&started.attempt_id).unwrap().status,
        StepStatus::Running
    );
    let take = state
        .get_voice_take_v1(&started.attempt_id)
        .unwrap()
        .unwrap();
    assert!(take.artifact.is_none());
    assert!(take.timing_artifact.is_none());

    let audio_path = workspace
        .data_root()
        .join("projects")
        .join(&project.id)
        .join("audio/takes/S01/take-0001.wav");
    let timing_path = workspace
        .data_root()
        .join("projects")
        .join(&project.id)
        .join("audio/takes/S01/take-0001.timing.json");
    assert!(!audio_path.exists());
    assert!(!timing_path.exists());
}

#[test]
fn corrupt_timing_transfer_fails_hash_before_local_success() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Corrupt Timing").unwrap();
    let job = create_voice_job(&state, &project.id, "input-hash-timing");
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let mut executor = FakeExecutor::default();

    let started = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    let audio = b"good audio";
    let expected_timing = timing_contract().to_json_bytes_v1().unwrap();
    let entry = bundle_entry(&started, &job.input_hash, audio, &expected_timing);
    executor
        .transfers
        .insert(format!("audio-{}", started.attempt_id), audio.to_vec());
    executor.transfers.insert(
        format!("timing-{}", started.attempt_id),
        b"corrupt timing bytes".to_vec(),
    );

    assert!(matches!(
        sync_remote_voice_artifact_bundle_v1(
            &mut state,
            &artifacts,
            &mut executor,
            &entry,
            temp.path().join("staging-corrupt"),
            serde_json::json!({}),
        ),
        Err(Error::ArtifactHashMismatch(_))
    ));
    assert_eq!(
        state.get_job(&job.job_id).unwrap().status,
        StepStatus::Running
    );
    assert!(state
        .get_voice_take_v1(&started.attempt_id)
        .unwrap()
        .unwrap()
        .artifact
        .is_none());
}

#[test]
fn legacy_single_audio_event_is_rejected_for_voice_take() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Bundle Required").unwrap();
    let job = create_voice_job(&state, &project.id, "input-hash-timing");
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let mut executor = FakeExecutor::default();

    let started = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    let audio = b"legacy audio only";
    let transfer_ref = format!("legacy-audio-{}", started.attempt_id);
    executor
        .transfers
        .insert(transfer_ref.clone(), audio.to_vec());
    let entry = ComputeRemoteJournalEntryV1 {
        schema: REMOTE_JOURNAL_SCHEMA_V1.to_owned(),
        version: 1,
        sequence: 3,
        provider_id: started.dispatch.provider_id.clone(),
        session_id: started.dispatch.session_id.clone(),
        job_id: job.job_id.clone(),
        attempt_id: started.attempt_id.clone(),
        input_hash: job.input_hash.clone(),
        event: ComputeRemoteJournalEventV1::ArtifactReady {
            artifact: ComputeRemoteArtifactV1 {
                artifact_type: "audio".to_owned(),
                output_uri: started.dispatch.output_uri.clone(),
                sha256: sha256(audio),
                size_bytes: audio.len() as u64,
                transfer_ref,
            },
        },
    };

    assert!(sync_remote_artifact(
        &mut state,
        &artifacts,
        &mut executor,
        &entry,
        temp.path().join("legacy-staging"),
        serde_json::json!({}),
    )
    .is_err());
    assert_eq!(
        state.get_job(&job.job_id).unwrap().status,
        StepStatus::Running
    );
}

#[test]
fn duplicate_bundle_delivery_is_idempotent_and_does_not_refetch() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Duplicate Bundle").unwrap();
    let job = create_voice_job(&state, &project.id, "input-hash-timing");
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let mut executor = FakeExecutor::default();

    let started = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    let audio = b"idempotent audio";
    let timing = timing_contract().to_json_bytes_v1().unwrap();
    register_transfers(&mut executor, &started, audio, &timing);
    let entry = bundle_entry(&started, &job.input_hash, audio, &timing);

    sync_remote_voice_artifact_bundle_v1(
        &mut state,
        &artifacts,
        &mut executor,
        &entry,
        temp.path().join("staging-first"),
        serde_json::json!({}),
    )
    .unwrap();
    let transfer_calls = executor.transfer_calls;

    let duplicate = sync_remote_voice_artifact_bundle_v1(
        &mut state,
        &artifacts,
        &mut executor,
        &entry,
        temp.path().join("staging-duplicate"),
        serde_json::json!({}),
    )
    .unwrap();

    assert!(matches!(
        duplicate,
        RemoteVoiceBundleSyncOutcomeV1::AlreadyCommitted(_)
    ));
    assert_eq!(executor.transfer_calls, transfer_calls);
    assert_eq!(state.list_voice_takes_v1(&job.job_id).unwrap().len(), 1);
}

#[test]
fn reconnect_recovers_audio_and_timing_bundle_before_regeneration() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Reconnect Timing").unwrap();
    let job = create_voice_job(&state, &project.id, "input-hash-timing");
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let mut executor = FakeExecutor::default();

    let started = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    let audio = b"completed while app was offline";
    let timing = timing_contract().to_json_bytes_v1().unwrap();
    register_transfers(&mut executor, &started, audio, &timing);
    executor.journal = vec![bundle_entry(&started, &job.input_hash, audio, &timing)];

    state.reconcile_interrupted_jobs().unwrap();
    assert_eq!(
        state.get_attempt(&started.attempt_id).unwrap().status,
        StepStatus::Retryable
    );

    let summary = reconcile_remote_session_v1(
        &mut state,
        &artifacts,
        &mut executor,
        "compute-provider",
        "session-timing",
        ComputeProviderConnectionState::Ready,
        temp.path().join("reconcile-staging"),
    )
    .unwrap();

    assert_eq!(summary.artifacts_recovered, 2);
    assert_eq!(
        state.get_job(&job.job_id).unwrap().status,
        StepStatus::Succeeded
    );
    assert!(artifacts
        .load_voice_timing_v1(&state, &started.attempt_id)
        .unwrap()
        .is_some());

    let transfers = executor.transfer_calls;
    let second = reconcile_remote_session_v1(
        &mut state,
        &artifacts,
        &mut executor,
        "compute-provider",
        "session-timing",
        ComputeProviderConnectionState::Ready,
        temp.path().join("reconcile-staging-2"),
    )
    .unwrap();
    assert_eq!(second.local_attempts_considered, 0);
    assert_eq!(executor.transfer_calls, transfers);
}

#[test]
fn selected_timing_follows_selected_take_not_newest_take() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Selected Timing").unwrap();
    let job = create_voice_job(&state, &project.id, "input-hash-timing");
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
    let first_audio = b"take one audio";
    let first_timing = timing_contract().to_json_bytes_v1().unwrap();
    register_transfers(&mut executor, &first, first_audio, &first_timing);
    let first_bundle = match sync_remote_voice_artifact_bundle_v1(
        &mut state,
        &artifacts,
        &mut executor,
        &bundle_entry(&first, &job.input_hash, first_audio, &first_timing),
        temp.path().join("first-staging"),
        serde_json::json!({}),
    )
    .unwrap()
    {
        RemoteVoiceBundleSyncOutcomeV1::Committed(bundle) => bundle,
        other => panic!("unexpected first bundle outcome {other:?}"),
    };

    state.request_voice_retake_v1(&job.job_id).unwrap();
    let second = dispatch_remote_voice_take_v1(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    let second_audio = b"take two audio";
    let mut second_timing_contract = timing_contract();
    second_timing_contract.duration_ms = 3000;
    second_timing_contract.cues[1].end_ms = 3000;
    let second_timing = second_timing_contract.to_json_bytes_v1().unwrap();
    register_transfers(&mut executor, &second, second_audio, &second_timing);
    let second_bundle = match sync_remote_voice_artifact_bundle_v1(
        &mut state,
        &artifacts,
        &mut executor,
        &bundle_entry(&second, &job.input_hash, second_audio, &second_timing),
        temp.path().join("second-staging"),
        serde_json::json!({}),
    )
    .unwrap()
    {
        RemoteVoiceBundleSyncOutcomeV1::Committed(bundle) => bundle,
        other => panic!("unexpected second bundle outcome {other:?}"),
    };

    assert_eq!(
        state
            .selected_voice_timing_artifact_v1(&job.job_id)
            .unwrap()
            .unwrap()
            .artifact_id,
        first_bundle.timing.artifact_id
    );

    state
        .select_voice_take_v1(&job.job_id, &second.attempt_id)
        .unwrap();
    assert_eq!(
        state
            .selected_voice_timing_artifact_v1(&job.job_id)
            .unwrap()
            .unwrap()
            .artifact_id,
        second_bundle.timing.artifact_id
    );
}

#[test]
fn timing_core_contract_is_provider_neutral_and_portable() {
    let audio = LogicalUri::parse("project://audio/takes/S04/take-0003.wav").unwrap();
    assert_eq!(
        voice_timing_output_uri_v1(&audio).unwrap(),
        LogicalUri::parse("project://audio/takes/S04/take-0003.timing.json").unwrap()
    );

    let source = include_str!("../src/voice_timing.rs").to_lowercase();
    for forbidden in [
        "kaggle",
        "notebook",
        "omnivoice",
        "c:\\",
        "/home/",
        "/users/",
    ] {
        assert!(
            !source.contains(forbidden),
            "timing core leaked provider/machine-specific term {forbidden}"
        );
    }
}

#[test]
fn artifact_bundle_contract_rejects_duplicate_members() {
    let audio = b"audio";
    let remote = ComputeRemoteArtifactV1 {
        artifact_type: "audio".to_owned(),
        output_uri: LogicalUri::parse("project://audio/a.wav").unwrap(),
        sha256: sha256(audio),
        size_bytes: audio.len() as u64,
        transfer_ref: "same".to_owned(),
    };
    let entry = ComputeRemoteJournalEntryV1 {
        schema: REMOTE_JOURNAL_SCHEMA_V1.to_owned(),
        version: 1,
        sequence: 1,
        provider_id: "compute-provider".to_owned(),
        session_id: "session-timing".to_owned(),
        job_id: "job".to_owned(),
        attempt_id: "attempt".to_owned(),
        input_hash: "hash".to_owned(),
        event: ComputeRemoteJournalEventV1::ArtifactBundleReady {
            artifacts: vec![remote.clone(), remote],
        },
    };
    assert!(entry.validate_v1().is_err());
}
