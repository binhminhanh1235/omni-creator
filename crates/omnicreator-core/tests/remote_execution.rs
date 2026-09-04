use std::{fs, path::Path};

use omnicreator_core::{
    dispatch_remote_job, parse_remote_journal_jsonl, sync_remote_artifact, ArtifactStore,
    ComputeDeviceSelectionV1, ComputeJobDispatchAckV1, ComputeJobDispatchV1,
    ComputeProviderExecution, ComputeRemoteArtifactV1, ComputeRemoteJournalEntryV1,
    ComputeRemoteJournalEventV1, ComputeRequirements, GpuJobPreparationV1,
    GpuQueueEligibilityStatusV1, GpuQueueEligibilityV1, LogicalUri,
    RemoteArtifactSyncOutcomeV1, RemoteComputeJobSpecV1, ResourceRequirement, StateStore,
    StepStatus, Workspace, REMOTE_JOURNAL_SCHEMA_V1,
};
use sha2::{Digest, Sha256};

#[derive(Default)]
struct FakeExecutor {
    dispatched: Vec<ComputeJobDispatchV1>,
    transfer_bytes: Vec<u8>,
    transfer_calls: usize,
    fail_dispatch: bool,
}

impl ComputeProviderExecution for FakeExecutor {
    fn dispatch_job(
        &mut self,
        dispatch: &ComputeJobDispatchV1,
    ) -> omnicreator_core::Result<ComputeJobDispatchAckV1> {
        if self.fail_dispatch {
            return Err(Error::InvalidContract(
                "fixture remote dispatch failure".to_owned(),
            ));
        }
        self.dispatched.push(dispatch.clone());
        Ok(ComputeJobDispatchAckV1 {
            job_id: dispatch.job_id.clone(),
            attempt_id: dispatch.attempt_id.clone(),
            remote_job_ref: format!("remote-{}", dispatch.job_id),
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
        fs::write(destination, &self.transfer_bytes)?;
        Ok(())
    }
}

fn selection() -> ComputeDeviceSelectionV1 {
    ComputeDeviceSelectionV1 {
        provider_id: "compute-provider".to_owned(),
        session_id: "session-p2".to_owned(),
        device_id: "gpu0".to_owned(),
        parallelizable: true,
        parallelism_group: "omnivoice-v3.2".to_owned(),
    }
}

fn spec(job_id: &str) -> RemoteComputeJobSpecV1 {
    RemoteComputeJobSpecV1 {
        job_id: job_id.to_owned(),
        operation: "tts.generate".to_owned(),
        plugin_payload: serde_json::json!({
            "segment_id": "S01",
            "voice": "warm-narrator-v4"
        }),
    }
}

fn preparation(job_id: &str) -> GpuJobPreparationV1 {
    GpuJobPreparationV1 {
        job_id: job_id.to_owned(),
        input_resolved: true,
        input_immutable: true,
        plugin_id: Some("omnivoice".to_owned()),
        provider_id: Some("compute-provider".to_owned()),
        model_id: Some("omnivoice-v3".to_owned()),
        model_version: Some("3.2".to_owned()),
        settings_fingerprint: Some("settings-v1".to_owned()),
        output_uri: Some(LogicalUri::parse("project://audio/S01.wav").unwrap()),
        approval_required: false,
        approval_complete: true,
        production_lock_required: false,
        preflight_required: false,
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

fn gpu_ready(job_id: &str) -> GpuQueueEligibilityV1 {
    GpuQueueEligibilityV1 {
        job_id: job_id.to_owned(),
        status: GpuQueueEligibilityStatusV1::GpuReady,
        reasons: Vec::new(),
        selection: Some(selection()),
    }
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn artifact_entry(
    job_id: &str,
    attempt_id: &str,
    input_hash: &str,
    bytes: &[u8],
) -> ComputeRemoteJournalEntryV1 {
    ComputeRemoteJournalEntryV1 {
        schema: REMOTE_JOURNAL_SCHEMA_V1.to_owned(),
        version: 1,
        sequence: 3,
        provider_id: "compute-provider".to_owned(),
        session_id: "session-p2".to_owned(),
        job_id: job_id.to_owned(),
        attempt_id: attempt_id.to_owned(),
        input_hash: input_hash.to_owned(),
        event: ComputeRemoteJournalEventV1::ArtifactReady {
            artifact: ComputeRemoteArtifactV1 {
                artifact_type: "audio".to_owned(),
                output_uri: LogicalUri::parse("project://audio/S01.wav").unwrap(),
                sha256: sha256(bytes),
                size_bytes: bytes.len() as u64,
                transfer_ref: "artifact/S01.wav".to_owned(),
            },
        },
    }
}

#[test]
fn remote_jsonl_journal_round_trips_and_rejects_non_monotonic_sequence() {
    let accepted = ComputeRemoteJournalEntryV1 {
        schema: REMOTE_JOURNAL_SCHEMA_V1.to_owned(),
        version: 1,
        sequence: 1,
        provider_id: "compute-provider".to_owned(),
        session_id: "session-p2".to_owned(),
        job_id: "job-1".to_owned(),
        attempt_id: "attempt-1".to_owned(),
        input_hash: "input-hash-1".to_owned(),
        event: ComputeRemoteJournalEventV1::Accepted,
    };
    let running = ComputeRemoteJournalEntryV1 {
        sequence: 2,
        event: ComputeRemoteJournalEventV1::Running,
        ..accepted.clone()
    };

    let jsonl = format!(
        "{}{}",
        accepted.to_json_line().unwrap(),
        running.to_json_line().unwrap()
    );
    assert_eq!(
        parse_remote_journal_jsonl(&jsonl).unwrap(),
        vec![accepted.clone(), running.clone()]
    );

    let invalid = format!(
        "{}{}",
        running.to_json_line().unwrap(),
        accepted.to_json_line().unwrap()
    );
    assert!(parse_remote_journal_jsonl(&invalid).is_err());
}

#[test]
fn provider_neutral_dispatch_starts_attempt_and_preserves_logical_job_identity() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Remote Dispatch").unwrap();
    let job = state
        .create_job(&project.id, "tts", "S01", "input-hash-001")
        .unwrap();
    let mut executor = FakeExecutor::default();

    let started = dispatch_remote_job(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();

    let persisted = state.get_job(&job.job_id).unwrap();
    assert_eq!(persisted.job_id, job.job_id);
    assert_eq!(persisted.status, StepStatus::Running);
    assert_eq!(executor.dispatched.len(), 1);
    assert_eq!(executor.dispatched[0].job_id, job.job_id);
    assert_eq!(executor.dispatched[0].attempt_id, started.attempt_id);
    assert_eq!(executor.dispatched[0].provider_id, "compute-provider");
    assert_eq!(executor.dispatched[0].session_id, "session-p2");
    assert_eq!(executor.dispatched[0].device_id, "gpu0");

    let attempts = state.list_attempts(&job.job_id).unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].attempt_id, started.attempt_id);
    assert_eq!(attempts[0].status, StepStatus::Running);
}

#[test]
fn remote_dispatch_refuses_non_gpu_ready_decision_without_creating_attempt() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Remote Dispatch Gate").unwrap();
    let job = state
        .create_job(&project.id, "tts", "S01", "input-hash-001")
        .unwrap();
    let mut executor = FakeExecutor::default();
    let not_ready = GpuQueueEligibilityV1 {
        job_id: job.job_id.clone(),
        status: GpuQueueEligibilityStatusV1::NotReady,
        reasons: Vec::new(),
        selection: None,
    };

    assert!(dispatch_remote_job(
        &mut state,
        &mut executor,
        &not_ready,
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .is_err());

    assert_eq!(state.get_job(&job.job_id).unwrap().status, StepStatus::Ready);
    assert!(state.list_attempts(&job.job_id).unwrap().is_empty());
    assert!(executor.dispatched.is_empty());
}

#[test]
fn failed_remote_dispatch_keeps_attempt_history_and_marks_job_retryable() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Remote Dispatch Failure").unwrap();
    let job = state
        .create_job(&project.id, "tts", "S01", "input-hash-001")
        .unwrap();
    let mut executor = FakeExecutor {
        fail_dispatch: true,
        ..FakeExecutor::default()
    };

    assert!(dispatch_remote_job(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .is_err());

    assert_eq!(
        state.get_job(&job.job_id).unwrap().status,
        StepStatus::Retryable
    );
    let attempts = state.list_attempts(&job.job_id).unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].status, StepStatus::Retryable);
    assert_eq!(
        attempts[0].error_code.as_deref(),
        Some("PROVIDER_UNAVAILABLE")
    );
}

#[test]
fn remote_artifact_is_transferred_verified_and_committed_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Remote Artifact").unwrap();
    let job = state
        .create_job(&project.id, "tts", "S01", "input-hash-001")
        .unwrap();

    let bytes = b"deterministic remote audio bytes".to_vec();
    let mut executor = FakeExecutor {
        transfer_bytes: bytes.clone(),
        ..FakeExecutor::default()
    };
    let started = dispatch_remote_job(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    let entry = artifact_entry(&job.job_id, &started.attempt_id, &job.input_hash, &bytes);
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();

    let outcome = sync_remote_artifact(
        &mut state,
        &artifacts,
        &mut executor,
        &entry,
        temp.path().join("runtime-staging"),
        serde_json::json!({"source": "remote-compute"}),
    )
    .unwrap();

    let committed = match outcome {
        RemoteArtifactSyncOutcomeV1::Committed(artifact) => artifact,
        RemoteArtifactSyncOutcomeV1::AlreadyCommitted(_) => {
            panic!("first delivery must commit a new artifact")
        }
    };

    let persisted_job = state.get_job(&job.job_id).unwrap();
    assert_eq!(persisted_job.status, StepStatus::Succeeded);
    assert_eq!(
        persisted_job.selected_attempt.as_deref(),
        Some(started.attempt_id.as_str())
    );
    assert_eq!(
        persisted_job.selected_artifact.as_deref(),
        Some(committed.artifact_id.as_str())
    );
    assert_eq!(
        state.get_attempt(&started.attempt_id).unwrap().status,
        StepStatus::Succeeded
    );
    assert!(artifacts.verify_artifact(&committed).unwrap());
    assert_eq!(
        fs::read(artifacts.resolve_artifact_path(&committed).unwrap()).unwrap(),
        bytes
    );
    assert_eq!(executor.transfer_calls, 1);

    let duplicate = sync_remote_artifact(
        &mut state,
        &artifacts,
        &mut executor,
        &entry,
        temp.path().join("runtime-staging"),
        serde_json::json!({"source": "remote-compute"}),
    )
    .unwrap();
    let duplicate_artifact = match duplicate {
        RemoteArtifactSyncOutcomeV1::AlreadyCommitted(artifact) => artifact,
        RemoteArtifactSyncOutcomeV1::Committed(_) => {
            panic!("duplicate delivery must be idempotent")
        }
    };
    assert_eq!(duplicate_artifact.artifact_id, committed.artifact_id);
    assert_eq!(executor.transfer_calls, 1);
    assert_eq!(state.list_attempts(&job.job_id).unwrap().len(), 1);
}

#[test]
fn corrupted_transfer_never_marks_attempt_or_job_succeeded() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Corrupt Remote Artifact").unwrap();
    let job = state
        .create_job(&project.id, "tts", "S01", "input-hash-001")
        .unwrap();

    let expected = b"expected remote bytes";
    let mut executor = FakeExecutor {
        transfer_bytes: b"corrupted in transit".to_vec(),
        ..FakeExecutor::default()
    };
    let started = dispatch_remote_job(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    let entry = artifact_entry(&job.job_id, &started.attempt_id, &job.input_hash, expected);
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();

    assert!(matches!(
        sync_remote_artifact(
            &mut state,
            &artifacts,
            &mut executor,
            &entry,
            temp.path().join("runtime-staging"),
            serde_json::json!({}),
        ),
        Err(Error::ArtifactHashMismatch(_))
    ));

    assert_eq!(
        state.get_job(&job.job_id).unwrap().status,
        StepStatus::Running
    );
    assert_eq!(
        state.get_attempt(&started.attempt_id).unwrap().status,
        StepStatus::Running
    );
    assert!(!workspace
        .data_root()
        .join("projects")
        .join(&project.id)
        .join("audio")
        .join("S01.wav")
        .exists());
}

#[test]
fn conflicting_duplicate_delivery_is_rejected_without_refetch() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Conflicting Duplicate").unwrap();
    let job = state
        .create_job(&project.id, "tts", "S01", "input-hash-001")
        .unwrap();

    let bytes = b"first remote artifact".to_vec();
    let mut executor = FakeExecutor {
        transfer_bytes: bytes.clone(),
        ..FakeExecutor::default()
    };
    let started = dispatch_remote_job(
        &mut state,
        &mut executor,
        &gpu_ready(&job.job_id),
        &preparation(&job.job_id),
        &spec(&job.job_id),
    )
    .unwrap();
    let entry = artifact_entry(&job.job_id, &started.attempt_id, &job.input_hash, &bytes);
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();

    sync_remote_artifact(
        &mut state,
        &artifacts,
        &mut executor,
        &entry,
        temp.path().join("runtime-staging"),
        serde_json::json!({}),
    )
    .unwrap();
    assert_eq!(executor.transfer_calls, 1);

    let conflicting_bytes = b"different bytes";
    let conflicting = artifact_entry(
        &job.job_id,
        &started.attempt_id,
        &job.input_hash,
        conflicting_bytes,
    );
    assert!(matches!(
        sync_remote_artifact(
            &mut state,
            &artifacts,
            &mut executor,
            &conflicting,
            temp.path().join("runtime-staging"),
            serde_json::json!({}),
        ),
        Err(Error::InvalidArtifact(_))
    ));
    assert_eq!(executor.transfer_calls, 1);
}
