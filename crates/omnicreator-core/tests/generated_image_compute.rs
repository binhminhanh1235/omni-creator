use std::{collections::BTreeMap, fs, path::{Path, PathBuf}};

use chrono::{TimeZone, Utc};
use omnicreator_core::{
    dispatch_generated_image_compute_v1, evaluate_gpu_queue, generated_image_remote_job_spec_v1,
    reconcile_remote_session_v1, scan_plugin_roots, sync_remote_artifact, ArtifactStore,
    CacheLookupV1, ComputeDeviceV1, ComputeJobDispatchAckV1, ComputeJobDispatchV1,
    ComputeProviderCapabilitiesV1, ComputeProviderConnectionState, ComputeProviderExecution,
    ComputeProviderSchedulingSnapshotV1, ComputeProviderSessionIdentityV1,
    ComputeProviderSessionV1, ComputeRemoteArtifactV1, ComputeRemoteJournalEntryV1,
    ComputeRemoteJournalEventV1, Error, GeneratedImageExecutionAvailabilityV1,
    GeneratedImageExecutionPolicyV1, GeneratedImagePreparationV1, GeneratedImageRequestV1,
    GeneratedImageResolutionV1, GeneratedImageStyleV1, GpuQueueEligibilityStatusV1,
    GpuQueueEligibilityV1, GpuReadinessFactsV1, LogicalUri, SceneIntentV1, StateStore,
    StepStatus, Workspace, REMOTE_JOURNAL_SCHEMA_V1, SCENE_INTENT_SCHEMA,
    SCENE_INTENT_SCHEMA_VERSION,
};
use sha2::{Digest, Sha256};

#[derive(Default)]
struct FakeExecutor {
    dispatched: Vec<ComputeJobDispatchV1>,
    journal: Vec<ComputeRemoteJournalEntryV1>,
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
                "fixture generated-image dispatch failure".to_owned(),
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
        provider_id: &str,
        session_id: &str,
        _after_sequence: Option<u64>,
    ) -> omnicreator_core::Result<Vec<ComputeRemoteJournalEntryV1>> {
        Ok(self
            .journal
            .iter()
            .filter(|entry| {
                entry.provider_id == provider_id && entry.session_id == session_id
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
        self.transfer_calls += 1;
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
        .plugins
        .into_iter()
        .find(|plugin| plugin.manifest.id == "generated-image-reference")
        .expect("generated image reference plugin must be discoverable")
}

fn scene() -> SceneIntentV1 {
    SceneIntentV1 {
        schema: SCENE_INTENT_SCHEMA.to_owned(),
        schema_version: SCENE_INTENT_SCHEMA_VERSION,
        id: "SC-P2C".to_owned(),
        segment_id: "S-P2C".to_owned(),
        narration: "A lantern glows beside a quiet path.".to_owned(),
        purpose: "Show safe guidance through uncertainty.".to_owned(),
        scene_type: "conceptual".to_owned(),
        emotion_before: Some("uncertain".to_owned()),
        emotion_after: Some("steady".to_owned()),
        duration_hint: Some(5.0),
        visual_ideas: vec!["warm lantern".to_owned(), "quiet path".to_owned()],
        search_queries: vec!["lantern quiet path".to_owned()],
        avoid: vec!["logos".to_owned(), "text overlay".to_owned()],
        continuity: BTreeMap::new(),
        aspect_ratio: "16:9".to_owned(),
    }
}

fn preparation() -> GeneratedImagePreparationV1 {
    let request = GeneratedImageRequestV1::from_scene_v1(
        scene(),
        GeneratedImageStyleV1 {
            preset: "reference".to_owned(),
            description: Some("warm editorial illustration".to_owned()),
        },
        GeneratedImageResolutionV1 {
            width: 1280,
            height: 720,
        },
        Some(7),
        BTreeMap::from([(
            "quality".to_owned(),
            serde_json::Value::String("balanced".to_owned()),
        )]),
    )
    .unwrap();

    GeneratedImagePreparationV1 {
        request,
        output_uri: Some(LogicalUri::parse("project://visual/SC-P2C.svg").unwrap()),
        provider_id: Some("compute-provider".to_owned()),
        model_id: Some("reference-svg".to_owned()),
        model_version: Some("1".to_owned()),
        approval_required: false,
        approval_complete: true,
        production_lock_required: false,
        gpu_execution_requested: true,
    }
}

fn gpu_ready(job_id: &str) -> GpuQueueEligibilityV1 {
    GpuQueueEligibilityV1 {
        job_id: job_id.to_owned(),
        status: GpuQueueEligibilityStatusV1::GpuReady,
        reasons: Vec::new(),
        selection: Some(omnicreator_core::ComputeDeviceSelectionV1 {
            provider_id: "compute-provider".to_owned(),
            session_id: "session-image".to_owned(),
            device_id: "gpu0".to_owned(),
            parallelizable: true,
            parallelism_group: "generated-image-reference-v1".to_owned(),
        }),
    }
}

fn availability(job_id: &str) -> GeneratedImageExecutionAvailabilityV1 {
    GeneratedImageExecutionAvailabilityV1 {
        plugin_runtime_ready: false,
        local_execution_ready: false,
        api: None,
        compute_provider: Some(gpu_ready(job_id)),
    }
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn journal_entry(
    job_id: &str,
    attempt_id: &str,
    input_hash: &str,
    sequence: u64,
    event: ComputeRemoteJournalEventV1,
) -> ComputeRemoteJournalEntryV1 {
    ComputeRemoteJournalEntryV1 {
        schema: REMOTE_JOURNAL_SCHEMA_V1.to_owned(),
        version: 1,
        sequence,
        provider_id: "compute-provider".to_owned(),
        session_id: "session-image".to_owned(),
        job_id: job_id.to_owned(),
        attempt_id: attempt_id.to_owned(),
        input_hash: input_hash.to_owned(),
        event,
    }
}

fn artifact_entry(
    job_id: &str,
    attempt_id: &str,
    input_hash: &str,
    bytes: &[u8],
    sequence: u64,
) -> ComputeRemoteJournalEntryV1 {
    journal_entry(
        job_id,
        attempt_id,
        input_hash,
        sequence,
        ComputeRemoteJournalEventV1::ArtifactReady {
            artifact: ComputeRemoteArtifactV1 {
                artifact_type: "image".to_owned(),
                output_uri: LogicalUri::parse("project://visual/SC-P2C.svg").unwrap(),
                sha256: sha256(bytes),
                size_bytes: bytes.len() as u64,
                transfer_ref: "artifact/SC-P2C.svg".to_owned(),
            },
        },
    )
}

fn create_job(state: &StateStore, project_id: &str, preparation: &GeneratedImagePreparationV1) -> omnicreator_core::Job {
    state
        .create_job(
            project_id,
            "visual.generate",
            &preparation.request.scene.id,
            &preparation.request.input_hash_v1().unwrap(),
        )
        .unwrap()
}

#[test]
fn generated_image_compute_preparation_uses_phase7_scheduler_contract() {
    let plugin = generated_plugin();
    let preparation = preparation();
    let job = omnicreator_core::Job {
        job_id: "job-image-p2c".to_owned(),
        project_id: "project-image-p2c".to_owned(),
        step: "visual.generate".to_owned(),
        unit: preparation.request.scene.id.clone(),
        status: StepStatus::Ready,
        input_hash: preparation.request.input_hash_v1().unwrap(),
        selected_attempt: None,
        selected_artifact: None,
    };
    let gpu = preparation
        .to_gpu_job_preparation_v1(&job.job_id, &plugin)
        .unwrap();

    assert_eq!(gpu.job_id, job.job_id);
    assert_eq!(gpu.requirements.min_vram_mb, Some(1024));
    assert_eq!(
        gpu.requirements.model_group.as_deref(),
        Some("generated-image-reference-v1")
    );
    assert_eq!(gpu.requirements.cost_metric.as_deref(), Some("megapixels"));

    let now = Utc.with_ymd_and_hms(2026, 9, 5, 8, 0, 0).unwrap();
    let provider = ComputeProviderSchedulingSnapshotV1 {
        state: ComputeProviderConnectionState::Ready,
        session: ComputeProviderSessionV1 {
            identity: ComputeProviderSessionIdentityV1 {
                provider_id: "compute-provider".to_owned(),
                session_id: "session-image".to_owned(),
            },
            connected_at: now,
            last_heartbeat_at: now,
            last_healthy_heartbeat_at: Some(now),
            capabilities: ComputeProviderCapabilitiesV1 {
                schema: "omnicreator.compute-capabilities".to_owned(),
                schema_version: 1,
                provider_id: "compute-provider".to_owned(),
                devices: vec![ComputeDeviceV1 {
                    id: "gpu0".to_owned(),
                    device_type: "gpu".to_owned(),
                    model: Some("NVIDIA T4".to_owned()),
                    memory_mb: Some(15_360),
                }],
                model_groups: vec!["generated-image-reference-v1".to_owned()],
                max_parallel_jobs: Some(1),
            },
        },
    };
    let facts = GpuReadinessFactsV1 {
        workflow_step_status: Some(StepStatus::Ready),
        dependencies_succeeded: true,
        production_locked: true,
        cache_lookup: CacheLookupV1::Miss,
    };

    let decision = evaluate_gpu_queue(&job, &facts, &gpu, &[provider], &[]).unwrap();
    assert_eq!(decision.status, GpuQueueEligibilityStatusV1::GpuReady);
    assert_eq!(decision.selection.unwrap().device_id, "gpu0");
}

#[test]
fn generated_image_compute_dispatch_preserves_semantic_payload_and_secret_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("P2C Dispatch").unwrap();
    let plugin = generated_plugin();
    let preparation = preparation();
    let job = create_job(&state, &project.id, &preparation);
    let mut executor = FakeExecutor::default();

    let started = dispatch_generated_image_compute_v1(
        &mut state,
        &mut executor,
        &plugin,
        &job.job_id,
        &preparation,
        &availability(&job.job_id),
        &GeneratedImageExecutionPolicyV1::default(),
    )
    .unwrap();

    assert_eq!(executor.dispatched.len(), 1);
    let dispatch = &executor.dispatched[0];
    assert_eq!(dispatch.job_id, job.job_id);
    assert_eq!(dispatch.attempt_id, started.attempt_id);
    assert_eq!(dispatch.input_hash, job.input_hash);
    assert_eq!(dispatch.plugin_id, "generated-image-reference");
    assert_eq!(dispatch.operation, "visual.generate");
    assert_eq!(dispatch.model_id, "reference-svg");
    assert_eq!(dispatch.model_version, "1");
    assert_eq!(
        dispatch.settings_fingerprint,
        preparation.request.settings_fingerprint
    );
    assert_eq!(
        dispatch.output_uri,
        preparation.output_uri.clone().unwrap()
    );
    assert_eq!(
        dispatch.plugin_payload,
        serde_json::to_value(&preparation.request).unwrap()
    );

    let wire = serde_json::to_string(dispatch).unwrap();
    for forbidden in [
        "SECRET_SENTINEL_P2C",
        "\"api_key\"",
        "\"access_token\"",
        "\"refresh_token\"",
        "\"credential_value\"",
    ] {
        assert!(!wire.contains(forbidden));
    }

    let spec = generated_image_remote_job_spec_v1(&job.job_id, &preparation).unwrap();
    assert_eq!(spec.operation, "visual.generate");
    assert_eq!(spec.plugin_payload["scene"]["id"], "SC-P2C");
    assert!(spec.plugin_payload["scene"].get("provider_id").is_none());
}

#[test]
fn generated_image_remote_success_reconciles_and_commits_verified_artifact() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("P2C Success").unwrap();
    let plugin = generated_plugin();
    let preparation = preparation();
    let job = create_job(&state, &project.id, &preparation);
    let bytes = b"<svg>verified generated image</svg>".to_vec();
    let mut executor = FakeExecutor {
        transfer_bytes: bytes.clone(),
        ..FakeExecutor::default()
    };
    let started = dispatch_generated_image_compute_v1(
        &mut state,
        &mut executor,
        &plugin,
        &job.job_id,
        &preparation,
        &availability(&job.job_id),
        &GeneratedImageExecutionPolicyV1::default(),
    )
    .unwrap();

    executor.journal = vec![
        journal_entry(
            &job.job_id,
            &started.attempt_id,
            &job.input_hash,
            1,
            ComputeRemoteJournalEventV1::Accepted,
        ),
        journal_entry(
            &job.job_id,
            &started.attempt_id,
            &job.input_hash,
            2,
            ComputeRemoteJournalEventV1::Running,
        ),
        artifact_entry(&job.job_id, &started.attempt_id, &job.input_hash, &bytes, 3),
    ];
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let summary = reconcile_remote_session_v1(
        &mut state,
        &artifacts,
        &mut executor,
        "compute-provider",
        "session-image",
        ComputeProviderConnectionState::Ready,
        temp.path().join("staging"),
    )
    .unwrap();

    assert_eq!(summary.artifacts_recovered, 1);
    let persisted = state.get_job(&job.job_id).unwrap();
    assert_eq!(persisted.status, StepStatus::Succeeded);
    assert_eq!(
        persisted.selected_attempt.as_deref(),
        Some(started.attempt_id.as_str())
    );
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
fn generated_image_hash_mismatch_is_fatal_and_never_commits_canonical_artifact() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("P2C Corrupt").unwrap();
    let plugin = generated_plugin();
    let preparation = preparation();
    let job = create_job(&state, &project.id, &preparation);
    let mut executor = FakeExecutor {
        transfer_bytes: b"corrupted bytes".to_vec(),
        ..FakeExecutor::default()
    };
    let started = dispatch_generated_image_compute_v1(
        &mut state,
        &mut executor,
        &plugin,
        &job.job_id,
        &preparation,
        &availability(&job.job_id),
        &GeneratedImageExecutionPolicyV1::default(),
    )
    .unwrap();
    let declared = b"<svg>expected bytes</svg>";
    let entry = artifact_entry(
        &job.job_id,
        &started.attempt_id,
        &job.input_hash,
        declared,
        1,
    );
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();

    assert!(matches!(
        sync_remote_artifact(
            &mut state,
            &artifacts,
            &mut executor,
            &entry,
            temp.path().join("staging"),
            serde_json::json!({"source": "generated-image-compute"}),
        ),
        Err(Error::ArtifactHashMismatch(_))
    ));

    assert_eq!(state.get_job(&job.job_id).unwrap().status, StepStatus::Fatal);
    let attempt = state.get_attempt(&started.attempt_id).unwrap();
    assert_eq!(attempt.status, StepStatus::Fatal);
    assert_eq!(
        attempt.error_code.as_deref(),
        Some("INVALID_REMOTE_ARTIFACT")
    );
    assert!(state.get_job(&job.job_id).unwrap().selected_artifact.is_none());
    assert!(!workspace
        .data_root()
        .join("projects")
        .join(&project.id)
        .join("visual")
        .join("SC-P2C.svg")
        .exists());
}

#[test]
fn generated_image_worker_loss_retries_same_job_then_selects_successful_attempt() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("P2C Retry").unwrap();
    let plugin = generated_plugin();
    let preparation = preparation();
    let job = create_job(&state, &project.id, &preparation);
    let input_hash = job.input_hash.clone();
    let mut executor = FakeExecutor::default();
    let first = dispatch_generated_image_compute_v1(
        &mut state,
        &mut executor,
        &plugin,
        &job.job_id,
        &preparation,
        &availability(&job.job_id),
        &GeneratedImageExecutionPolicyV1::default(),
    )
    .unwrap();

    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let summary = reconcile_remote_session_v1(
        &mut state,
        &artifacts,
        &mut executor,
        "compute-provider",
        "session-image",
        ComputeProviderConnectionState::Lost,
        temp.path().join("staging"),
    )
    .unwrap();
    assert_eq!(summary.attempts_marked_retryable, 1);

    let bytes = b"<svg>retry success</svg>".to_vec();
    executor.transfer_bytes = bytes.clone();
    let second = dispatch_generated_image_compute_v1(
        &mut state,
        &mut executor,
        &plugin,
        &job.job_id,
        &preparation,
        &availability(&job.job_id),
        &GeneratedImageExecutionPolicyV1::default(),
    )
    .unwrap();
    let entry = artifact_entry(&job.job_id, &second.attempt_id, &input_hash, &bytes, 2);
    sync_remote_artifact(
        &mut state,
        &artifacts,
        &mut executor,
        &entry,
        temp.path().join("staging"),
        serde_json::json!({"source": "generated-image-compute"}),
    )
    .unwrap();

    let attempts = state.list_attempts(&job.job_id).unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].attempt_id, first.attempt_id);
    assert_eq!(attempts[0].status, StepStatus::Retryable);
    assert_eq!(attempts[1].attempt_id, second.attempt_id);
    assert_eq!(attempts[1].status, StepStatus::Succeeded);
    assert_ne!(first.attempt_id, second.attempt_id);

    let persisted = state.get_job(&job.job_id).unwrap();
    assert_eq!(persisted.input_hash, input_hash);
    assert_eq!(persisted.status, StepStatus::Succeeded);
    assert_eq!(
        persisted.selected_attempt.as_deref(),
        Some(second.attempt_id.as_str())
    );
    let selected = state
        .get_artifact(persisted.selected_artifact.as_deref().unwrap())
        .unwrap();
    assert!(artifacts.verify_artifact(&selected).unwrap());
}

#[test]
fn generated_image_restart_reconciliation_recovers_remote_completion_idempotently() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("P2C Restart").unwrap();
    let plugin = generated_plugin();
    let preparation = preparation();
    let job = create_job(&state, &project.id, &preparation);
    let bytes = b"<svg>completed during restart</svg>".to_vec();
    let mut executor = FakeExecutor {
        transfer_bytes: bytes.clone(),
        ..FakeExecutor::default()
    };
    let started = dispatch_generated_image_compute_v1(
        &mut state,
        &mut executor,
        &plugin,
        &job.job_id,
        &preparation,
        &availability(&job.job_id),
        &GeneratedImageExecutionPolicyV1::default(),
    )
    .unwrap();

    state.reconcile_interrupted_jobs().unwrap();
    assert_eq!(state.get_job(&job.job_id).unwrap().status, StepStatus::Retryable);
    executor.journal = vec![artifact_entry(
        &job.job_id,
        &started.attempt_id,
        &job.input_hash,
        &bytes,
        1,
    )];
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();

    let first = reconcile_remote_session_v1(
        &mut state,
        &artifacts,
        &mut executor,
        "compute-provider",
        "session-image",
        ComputeProviderConnectionState::Ready,
        temp.path().join("reconcile"),
    )
    .unwrap();
    assert_eq!(first.artifacts_recovered, 1);

    let persisted = state.get_job(&job.job_id).unwrap();
    let selected_id = persisted.selected_artifact.clone().unwrap();
    assert_eq!(persisted.status, StepStatus::Succeeded);
    assert_eq!(state.list_attempts(&job.job_id).unwrap().len(), 1);

    let second = reconcile_remote_session_v1(
        &mut state,
        &artifacts,
        &mut executor,
        "compute-provider",
        "session-image",
        ComputeProviderConnectionState::Ready,
        temp.path().join("reconcile"),
    )
    .unwrap();
    assert_eq!(second.artifacts_recovered, 0);
    assert_eq!(state.get_job(&job.job_id).unwrap().selected_artifact, Some(selected_id));
    assert_eq!(state.list_attempts(&job.job_id).unwrap().len(), 1);
}

#[test]
fn generated_image_provider_unavailable_preserves_job_and_marks_attempt_retryable() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("P2C Provider Failure").unwrap();
    let plugin = generated_plugin();
    let preparation = preparation();
    let job = create_job(&state, &project.id, &preparation);
    let mut executor = FakeExecutor {
        fail_dispatch: true,
        ..FakeExecutor::default()
    };

    assert!(dispatch_generated_image_compute_v1(
        &mut state,
        &mut executor,
        &plugin,
        &job.job_id,
        &preparation,
        &availability(&job.job_id),
        &GeneratedImageExecutionPolicyV1::default(),
    )
    .is_err());

    let persisted = state.get_job(&job.job_id).unwrap();
    assert_eq!(persisted.job_id, job.job_id);
    assert_eq!(persisted.status, StepStatus::Retryable);
    let attempts = state.list_attempts(&job.job_id).unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].status, StepStatus::Retryable);
    assert_eq!(attempts[0].error_code.as_deref(), Some("PROVIDER_UNAVAILABLE"));
}
