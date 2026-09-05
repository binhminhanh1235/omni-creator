use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use omnicreator_core::{
    dispatch_generated_image_compute_provider_v1, reconcile_remote_session_v1, scan_plugin_roots,
    ArtifactStore, ComputeDeviceSelectionV1, ComputeJobDispatchAckV1, ComputeJobDispatchV1,
    ComputeProviderConnectionState, ComputeProviderExecution, ComputeRemoteArtifactV1,
    ComputeRemoteJournalEntryV1, ComputeRemoteJournalEventV1, Error,
    GeneratedImageExecutionDecisionStatusV1, GeneratedImageExecutionDecisionV1,
    GeneratedImageExecutionTargetV1, GeneratedImagePreparationV1, GeneratedImageRequestV1,
    GeneratedImageResolutionV1, GeneratedImageStyleV1, GpuQueueEligibilityStatusV1,
    GpuQueueEligibilityV1, LogicalUri, SceneIntentV1, StateStore, StepStatus, Workspace,
    GENERATED_IMAGE_EXECUTION_DECISION_SCHEMA_V1, REMOTE_JOURNAL_SCHEMA_V1, SCENE_INTENT_SCHEMA,
    SCENE_INTENT_SCHEMA_VERSION,
};
use sha2::{Digest, Sha256};

#[derive(Default)]
struct FakeExecutor {
    dispatched: Vec<ComputeJobDispatchV1>,
    journal: Vec<ComputeRemoteJournalEntryV1>,
    transfer_bytes: Vec<u8>,
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
                    && after_sequence.map_or(true, |sequence| entry.sequence > sequence)
            })
            .cloned()
            .collect())
    }

    fn transfer_artifact(
        &mut self,
        _provider_id: &str,
        _session_id: &str,
        _transfer_ref: &str,
        destination: &Path,
    ) -> omnicreator_core::Result<()> {
        fs::write(destination, &self.transfer_bytes)?;
        Ok(())
    }
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

fn scene() -> SceneIntentV1 {
    SceneIntentV1 {
        schema: SCENE_INTENT_SCHEMA.to_owned(),
        schema_version: SCENE_INTENT_SCHEMA_VERSION,
        id: "SC01".to_owned(),
        segment_id: "S01".to_owned(),
        narration: "A careful repair restores a weathered wooden gate.".to_owned(),
        purpose: "Show patient restoration.".to_owned(),
        scene_type: "conceptual".to_owned(),
        emotion_before: Some("worn".to_owned()),
        emotion_after: Some("hopeful".to_owned()),
        duration_hint: Some(6.0),
        visual_ideas: vec!["hands repairing a wooden gate".to_owned()],
        search_queries: vec!["repairing wooden gate close up".to_owned()],
        avoid: vec!["logos".to_owned(), "text overlays".to_owned()],
        continuity: BTreeMap::new(),
        aspect_ratio: "16:9".to_owned(),
    }
}

fn request() -> GeneratedImageRequestV1 {
    GeneratedImageRequestV1::from_scene_v1(
        scene(),
        GeneratedImageStyleV1 {
            preset: "remote-fixture".to_owned(),
            description: Some("warm documentary still".to_owned()),
        },
        GeneratedImageResolutionV1 {
            width: 64,
            height: 64,
        },
        Some(17),
        BTreeMap::new(),
    )
    .unwrap()
}

fn preparation() -> GeneratedImagePreparationV1 {
    GeneratedImagePreparationV1 {
        request: request(),
        output_uri: Some(LogicalUri::parse("project://visual/SC01.png").unwrap()),
        provider_id: Some("compute-provider".to_owned()),
        model_id: Some("reference-svg".to_owned()),
        model_version: Some("1".to_owned()),
        approval_required: false,
        approval_complete: true,
        production_lock_required: false,
        gpu_execution_requested: true,
    }
}

fn compute_decision() -> GeneratedImageExecutionDecisionV1 {
    GeneratedImageExecutionDecisionV1 {
        schema: GENERATED_IMAGE_EXECUTION_DECISION_SCHEMA_V1.to_owned(),
        version: 1,
        status: GeneratedImageExecutionDecisionStatusV1::Ready,
        target: Some(GeneratedImageExecutionTargetV1::ComputeProvider),
        preflight_issues: Vec::new(),
        rejections: Vec::new(),
    }
}

fn gpu_ready(job_id: &str, session_id: &str, device_id: &str) -> GpuQueueEligibilityV1 {
    GpuQueueEligibilityV1 {
        job_id: job_id.to_owned(),
        status: GpuQueueEligibilityStatusV1::GpuReady,
        reasons: Vec::new(),
        selection: Some(ComputeDeviceSelectionV1 {
            provider_id: "compute-provider".to_owned(),
            session_id: session_id.to_owned(),
            device_id: device_id.to_owned(),
            parallelizable: true,
            parallelism_group: "generated-image-reference-v1".to_owned(),
        }),
    }
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn artifact_entry(
    job_id: &str,
    attempt_id: &str,
    input_hash: &str,
    session_id: &str,
    bytes: &[u8],
) -> ComputeRemoteJournalEntryV1 {
    ComputeRemoteJournalEntryV1 {
        schema: REMOTE_JOURNAL_SCHEMA_V1.to_owned(),
        version: 1,
        sequence: 1,
        provider_id: "compute-provider".to_owned(),
        session_id: session_id.to_owned(),
        job_id: job_id.to_owned(),
        attempt_id: attempt_id.to_owned(),
        input_hash: input_hash.to_owned(),
        event: ComputeRemoteJournalEventV1::ArtifactReady {
            artifact: ComputeRemoteArtifactV1 {
                artifact_type: "image".to_owned(),
                output_uri: LogicalUri::parse("project://visual/SC01.png").unwrap(),
                sha256: sha256(bytes),
                size_bytes: bytes.len() as u64,
                transfer_ref: "artifact/SC01.png".to_owned(),
            },
        },
    }
}

#[test]
fn generated_image_dispatch_reuses_canonical_compute_contract_and_semantic_payload() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Generated Image Remote").unwrap();
    let preparation = preparation();
    let job = state
        .create_job(
            &project.id,
            "visual.generate",
            &preparation.request.scene.id,
            &preparation.request.input_hash_v1().unwrap(),
        )
        .unwrap();
    let mut executor = FakeExecutor::default();

    let started = dispatch_generated_image_compute_provider_v1(
        &mut state,
        &mut executor,
        &compute_decision(),
        &gpu_ready(&job.job_id, "session-a", "gpu1"),
        &generated_plugin(),
        &job.job_id,
        &preparation,
    )
    .unwrap();

    assert_eq!(executor.dispatched.len(), 1);
    let dispatch = &executor.dispatched[0];
    assert_eq!(dispatch.job_id, job.job_id);
    assert_eq!(dispatch.attempt_id, started.attempt_id);
    assert_eq!(dispatch.operation, "visual.generate");
    assert_eq!(dispatch.plugin_id, "generated-image-reference");
    assert_eq!(dispatch.provider_id, "compute-provider");
    assert_eq!(dispatch.session_id, "session-a");
    assert_eq!(dispatch.device_id, "gpu1");
    assert_eq!(
        dispatch.plugin_payload,
        serde_json::to_value(&preparation.request).unwrap()
    );
    assert_eq!(dispatch.plugin_payload["scene"]["id"], "SC01");
    assert!(dispatch.plugin_payload["scene"]
        .get("provider_id")
        .is_none());
    assert_eq!(
        state.get_job(&job.job_id).unwrap().status,
        StepStatus::Running
    );
    assert_eq!(state.list_attempts(&job.job_id).unwrap().len(), 1);
}

#[test]
fn generated_image_remote_dispatch_requires_compute_target_before_attempt_creation() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Generated Image Remote Gate").unwrap();
    let preparation = preparation();
    let job = state
        .create_job(
            &project.id,
            "visual.generate",
            &preparation.request.scene.id,
            &preparation.request.input_hash_v1().unwrap(),
        )
        .unwrap();
    let mut executor = FakeExecutor::default();
    let mut decision = compute_decision();
    decision.target = Some(GeneratedImageExecutionTargetV1::Api);

    let error = dispatch_generated_image_compute_provider_v1(
        &mut state,
        &mut executor,
        &decision,
        &gpu_ready(&job.job_id, "session-a", "gpu0"),
        &generated_plugin(),
        &job.job_id,
        &preparation,
    )
    .unwrap_err();

    assert!(error.to_string().contains("compute_provider"));
    assert!(state.list_attempts(&job.job_id).unwrap().is_empty());
    assert!(executor.dispatched.is_empty());
}

#[test]
fn generated_image_reconciliation_recovers_verified_remote_artifact_after_local_restart() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state
        .create_project("Generated Image Remote Resume")
        .unwrap();
    let preparation = preparation();
    let job = state
        .create_job(
            &project.id,
            "visual.generate",
            &preparation.request.scene.id,
            &preparation.request.input_hash_v1().unwrap(),
        )
        .unwrap();
    let bytes = b"\x89PNG\r\n\x1a\nremote-generated-image".to_vec();
    let mut executor = FakeExecutor {
        transfer_bytes: bytes.clone(),
        ..FakeExecutor::default()
    };
    let started = dispatch_generated_image_compute_provider_v1(
        &mut state,
        &mut executor,
        &compute_decision(),
        &gpu_ready(&job.job_id, "session-a", "gpu0"),
        &generated_plugin(),
        &job.job_id,
        &preparation,
    )
    .unwrap();

    state.reconcile_interrupted_jobs().unwrap();
    assert_eq!(
        state.get_job(&job.job_id).unwrap().status,
        StepStatus::Retryable
    );
    executor.journal = vec![artifact_entry(
        &job.job_id,
        &started.attempt_id,
        &job.input_hash,
        "session-a",
        &bytes,
    )];
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();

    let summary = reconcile_remote_session_v1(
        &mut state,
        &artifacts,
        &mut executor,
        "compute-provider",
        "session-a",
        ComputeProviderConnectionState::Ready,
        temp.path().join("reconcile-staging"),
    )
    .unwrap();

    assert_eq!(summary.artifacts_recovered, 1);
    let persisted = state.get_job(&job.job_id).unwrap();
    assert_eq!(persisted.job_id, job.job_id);
    assert_eq!(persisted.status, StepStatus::Succeeded);
    assert_eq!(
        persisted.selected_attempt.as_deref(),
        Some(started.attempt_id.as_str())
    );
    assert_eq!(state.list_attempts(&job.job_id).unwrap().len(), 1);

    let artifact = state
        .get_artifact(persisted.selected_artifact.as_deref().unwrap())
        .unwrap();
    assert_eq!(artifact.artifact_type, "image");
    assert!(artifacts.verify_artifact(&artifact).unwrap());
    assert_eq!(
        fs::read(artifacts.resolve_artifact_path(&artifact).unwrap()).unwrap(),
        bytes
    );
}

#[test]
fn generated_image_worker_loss_requeues_same_logical_job_and_retry_appends_attempt() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Generated Image Worker Loss").unwrap();
    let preparation = preparation();
    let job = state
        .create_job(
            &project.id,
            "visual.generate",
            &preparation.request.scene.id,
            &preparation.request.input_hash_v1().unwrap(),
        )
        .unwrap();
    let mut executor = FakeExecutor::default();
    let first = dispatch_generated_image_compute_provider_v1(
        &mut state,
        &mut executor,
        &compute_decision(),
        &gpu_ready(&job.job_id, "session-a", "gpu0"),
        &generated_plugin(),
        &job.job_id,
        &preparation,
    )
    .unwrap();
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();

    let summary = reconcile_remote_session_v1(
        &mut state,
        &artifacts,
        &mut executor,
        "compute-provider",
        "session-a",
        ComputeProviderConnectionState::Lost,
        temp.path().join("reconcile-staging"),
    )
    .unwrap();
    assert_eq!(summary.attempts_marked_retryable, 1);
    assert_eq!(
        state.get_job(&job.job_id).unwrap().status,
        StepStatus::Retryable
    );

    let retry_snapshot = state
        .gpu_workbench_queue_snapshot_v1(std::slice::from_ref(&project.id))
        .unwrap();
    assert_eq!(retry_snapshot.retryable.len(), 1);
    assert!(retry_snapshot.running.is_empty());
    assert_eq!(retry_snapshot.retryable[0].job.job_id, job.job_id);
    assert_eq!(retry_snapshot.retryable[0].attempts.len(), 1);
    assert_eq!(
        retry_snapshot.retryable[0].attempts[0]
            .error_code
            .as_deref(),
        Some("WORKER_LOST")
    );

    let second = dispatch_generated_image_compute_provider_v1(
        &mut state,
        &mut executor,
        &compute_decision(),
        &gpu_ready(&job.job_id, "session-b", "gpu1"),
        &generated_plugin(),
        &job.job_id,
        &preparation,
    )
    .unwrap();

    let attempts = state.list_attempts(&job.job_id).unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].attempt_id, first.attempt_id);
    assert_eq!(attempts[0].status, StepStatus::Retryable);
    assert_eq!(attempts[0].error_code.as_deref(), Some("WORKER_LOST"));
    assert_eq!(attempts[1].attempt_id, second.attempt_id);
    assert_eq!(attempts[1].status, StepStatus::Running);
    assert_eq!(state.get_job(&job.job_id).unwrap().job_id, job.job_id);

    let resumed_snapshot = state
        .gpu_workbench_queue_snapshot_v1(std::slice::from_ref(&project.id))
        .unwrap();
    assert!(resumed_snapshot.retryable.is_empty());
    assert_eq!(resumed_snapshot.running.len(), 1);
    assert_eq!(resumed_snapshot.running[0].job.job_id, job.job_id);
    assert_eq!(resumed_snapshot.running[0].attempts.len(), 2);
    assert_eq!(
        resumed_snapshot.running[0].attempts[0]
            .error_code
            .as_deref(),
        Some("WORKER_LOST")
    );
    assert_eq!(
        resumed_snapshot.running[0].attempts[1].attempt_id,
        second.attempt_id
    );
}

#[test]
fn generated_image_remote_hash_mismatch_never_commits_success() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state
        .create_project("Generated Image Remote Hash Gate")
        .unwrap();
    let preparation = preparation();
    let job = state
        .create_job(
            &project.id,
            "visual.generate",
            &preparation.request.scene.id,
            &preparation.request.input_hash_v1().unwrap(),
        )
        .unwrap();
    let expected = b"expected-generated-image";
    let mut executor = FakeExecutor {
        transfer_bytes: b"corrupted-generated-image".to_vec(),
        ..FakeExecutor::default()
    };
    let started = dispatch_generated_image_compute_provider_v1(
        &mut state,
        &mut executor,
        &compute_decision(),
        &gpu_ready(&job.job_id, "session-a", "gpu0"),
        &generated_plugin(),
        &job.job_id,
        &preparation,
    )
    .unwrap();
    executor.journal = vec![artifact_entry(
        &job.job_id,
        &started.attempt_id,
        &job.input_hash,
        "session-a",
        expected,
    )];
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();

    let error = reconcile_remote_session_v1(
        &mut state,
        &artifacts,
        &mut executor,
        "compute-provider",
        "session-a",
        ComputeProviderConnectionState::Ready,
        temp.path().join("reconcile-staging"),
    )
    .unwrap_err();

    assert!(matches!(error, Error::ArtifactHashMismatch(_)));
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
        .join("visual")
        .join("SC01.png")
        .exists());
}
