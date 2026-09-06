use std::path::Path;

use chrono::{TimeZone, Utc};
use omnicreator_core::{
    compile_creator_workflow_plan_v1, default_segment_tts_compute_requirements_v1,
    dispatch_creator_voice_burst_v1, initial_studio_pack_catalog_v1,
    materialize_creator_workflow_plan_v1, plan_creator_voice_orchestration_v1, ComputeDeviceV1,
    ComputeJobDispatchAckV1, ComputeJobDispatchV1, ComputeProviderCapabilitiesV1,
    ComputeProviderConnectionState, ComputeProviderExecution, ComputeProviderSchedulingSnapshotV1,
    ComputeProviderSessionIdentityV1, ComputeProviderSessionV1, ComputeRemoteJournalEntryV1,
    CreatorContentV1, CreatorInputV1, CreatorVoiceRuntimeV1, Error, PronunciationRuleV1, Result,
    SegmentTtsLockStateV1, SegmentV1, StateStore, StepStatus, VoiceDirectionV1, VoiceIdentityV1,
    VoiceModelIdentityV1, Workspace, CREATOR_CONTENT_SCHEMA_V1, CREATOR_CONTENT_VERSION_V1,
    CREATOR_STEP_CONTENT_PREPARE_V1, CREATOR_STEP_PRODUCTION_PACK_V1,
    CREATOR_STEP_VOICE_PREPARE_V1, CREATOR_TTS_STEP_V1, SEGMENT_SCHEMA, SEGMENT_SCHEMA_VERSION,
};

struct Fixture {
    _temp: tempfile::TempDir,
    workspace: Workspace,
    store: StateStore,
    project_id: String,
    content: CreatorContentV1,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let store = StateStore::open(workspace.sqlite_path()).unwrap();
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let project = store
        .create_project_with_studio_pack("P3 voice fixture", Some(&pack.id))
        .unwrap();
    let plan = compile_creator_workflow_plan_v1(&project, &pack).unwrap();
    materialize_creator_workflow_plan_v1(&store, &plan).unwrap();

    let content_step = store
        .list_project_steps(&project.id)
        .unwrap()
        .into_iter()
        .find(|step| step.step == CREATOR_STEP_CONTENT_PREPARE_V1)
        .unwrap();
    store
        .set_step_status(&content_step.step_id, StepStatus::Succeeded)
        .unwrap();
    store.refresh_ready_steps(&project.id).unwrap();

    let content = CreatorContentV1 {
        schema: CREATOR_CONTENT_SCHEMA_V1.to_owned(),
        schema_version: CREATOR_CONTENT_VERSION_V1,
        project_id: project.id.clone(),
        source: CreatorInputV1::script("First segment.\n\nSecond segment."),
        script: "First segment.\n\nSecond segment.".to_owned(),
        segments: vec![
            segment("S001", 1, "First segment."),
            segment("S002", 2, "Second segment."),
        ],
    };
    content.validate_v1().unwrap();

    Fixture {
        _temp: temp,
        workspace,
        store,
        project_id: project.id,
        content,
    }
}

fn segment(id: &str, order: u32, text: &str) -> SegmentV1 {
    SegmentV1 {
        schema: SEGMENT_SCHEMA.to_owned(),
        schema_version: SEGMENT_SCHEMA_VERSION,
        id: id.to_owned(),
        order,
        text: text.to_owned(),
        voice_direction: VoiceDirectionV1 {
            tone: Some("warm".to_owned()),
            pace: Some("measured".to_owned()),
            tags: vec!["devotional".to_owned()],
        },
    }
}

fn runtime(settings: &str) -> CreatorVoiceRuntimeV1 {
    CreatorVoiceRuntimeV1 {
        plugin_id: "omnivoice".to_owned(),
        provider_id: "kaggle".to_owned(),
        voice: VoiceIdentityV1 {
            voice_id: "narrator-warm".to_owned(),
            voice_version: "1".to_owned(),
        },
        model: VoiceModelIdentityV1 {
            model_id: "omnivoice-v3".to_owned(),
            model_version: "3.2".to_owned(),
        },
        settings_fingerprint: settings.to_owned(),
        pronunciation_rules: vec![PronunciationRuleV1 {
            written: "OmniCreator".to_owned(),
            pronunciation: "Omni Creator".to_owned(),
        }],
        locks: SegmentTtsLockStateV1 {
            normalization_locked: true,
            pronunciation_locked: true,
        },
        approval_required: false,
        approval_complete: true,
        production_lock_required: false,
        gpu_execution_requested: true,
        requirements: default_segment_tts_compute_requirements_v1("omnivoice", 12_000),
    }
}

fn provider() -> ComputeProviderSchedulingSnapshotV1 {
    let now = Utc.with_ymd_and_hms(2026, 9, 6, 10, 0, 0).unwrap();
    ComputeProviderSchedulingSnapshotV1 {
        state: ComputeProviderConnectionState::Ready,
        session: ComputeProviderSessionV1 {
            identity: ComputeProviderSessionIdentityV1 {
                provider_id: "kaggle".to_owned(),
                session_id: "session-p3".to_owned(),
            },
            connected_at: now,
            last_heartbeat_at: now,
            last_healthy_heartbeat_at: Some(now),
            capabilities: ComputeProviderCapabilitiesV1 {
                schema: "omnicreator.compute-capabilities".to_owned(),
                schema_version: 1,
                provider_id: "kaggle".to_owned(),
                devices: vec![
                    ComputeDeviceV1 {
                        id: "gpu0".to_owned(),
                        device_type: "gpu".to_owned(),
                        model: Some("NVIDIA T4".to_owned()),
                        memory_mb: Some(15_360),
                    },
                    ComputeDeviceV1 {
                        id: "gpu1".to_owned(),
                        device_type: "gpu".to_owned(),
                        model: Some("NVIDIA T4".to_owned()),
                        memory_mb: Some(15_360),
                    },
                ],
                model_groups: vec!["omnivoice".to_owned()],
                max_parallel_jobs: Some(2),
            },
        },
    }
}

#[derive(Default)]
struct FakeExecutor {
    fail: bool,
    dispatched: Vec<ComputeJobDispatchV1>,
}

impl ComputeProviderExecution for FakeExecutor {
    fn dispatch_job(&mut self, dispatch: &ComputeJobDispatchV1) -> Result<ComputeJobDispatchAckV1> {
        self.dispatched.push(dispatch.clone());
        if self.fail {
            return Err(Error::InvalidContract(
                "fixture provider unavailable".to_owned(),
            ));
        }
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
    ) -> Result<Vec<ComputeRemoteJournalEntryV1>> {
        Ok(Vec::new())
    }

    fn transfer_artifact(
        &mut self,
        _provider_id: &str,
        _session_id: &str,
        _transfer_ref: &str,
        _destination: &Path,
    ) -> Result<()> {
        Ok(())
    }
}

#[test]
fn p3_materializes_segment_tts_jobs_and_voice_parent_waits_for_them() {
    let mut fixture = fixture();
    let artifacts = omnicreator_core::ArtifactStore::new(fixture.workspace.data_root()).unwrap();
    let plan = plan_creator_voice_orchestration_v1(
        &mut fixture.store,
        &artifacts,
        &fixture.content,
        &runtime("voice-settings-v1"),
        &[provider()],
    )
    .unwrap();

    assert_eq!(plan.segments.len(), 2);
    assert_eq!(plan.burst.scheduled_job_count(), 2);
    assert!(plan.burst.blocked.is_empty());
    assert!(plan.completed_segment_ids.is_empty());
    assert!(plan.in_flight_job_ids.is_empty());

    let steps = fixture
        .store
        .list_project_steps(&fixture.project_id)
        .unwrap();
    let tts = steps
        .iter()
        .filter(|step| step.step == CREATOR_TTS_STEP_V1)
        .collect::<Vec<_>>();
    assert_eq!(tts.len(), 2);
    assert!(tts.iter().all(|step| step.status == StepStatus::Ready));

    let voice = steps
        .iter()
        .find(|step| step.step == CREATOR_STEP_VOICE_PREPARE_V1)
        .unwrap();
    assert_eq!(voice.status, StepStatus::NotReady);

    let first_job_ids = plan
        .segments
        .iter()
        .map(|segment| segment.job.job_id.clone())
        .collect::<Vec<_>>();
    let second = plan_creator_voice_orchestration_v1(
        &mut fixture.store,
        &artifacts,
        &fixture.content,
        &runtime("voice-settings-v1"),
        &[provider()],
    )
    .unwrap();
    assert_eq!(
        first_job_ids,
        second
            .segments
            .iter()
            .map(|segment| segment.job.job_id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        fixture
            .store
            .list_project_jobs(&fixture.project_id)
            .unwrap()
            .into_iter()
            .filter(|job| job.step == CREATOR_TTS_STEP_V1)
            .count(),
        2
    );
}

#[test]
fn p3_retry_preserves_voice_take_history_and_uses_next_take_uri() {
    let mut fixture = fixture();
    let artifacts = omnicreator_core::ArtifactStore::new(fixture.workspace.data_root()).unwrap();
    let first = plan_creator_voice_orchestration_v1(
        &mut fixture.store,
        &artifacts,
        &fixture.content,
        &runtime("voice-settings-v1"),
        &[provider()],
    )
    .unwrap();

    let mut failing = FakeExecutor {
        fail: true,
        ..Default::default()
    };
    let failed = dispatch_creator_voice_burst_v1(&mut fixture.store, &mut failing, &first).unwrap();
    assert_eq!(failed.failures.len(), 2);
    assert!(failed.dispatched.is_empty());

    for segment in &first.segments {
        assert_eq!(
            fixture.store.get_job(&segment.job.job_id).unwrap().status,
            StepStatus::Retryable
        );
        assert_eq!(
            fixture
                .store
                .list_voice_takes_v1(&segment.job.job_id)
                .unwrap()
                .len(),
            1
        );
    }

    let retry = plan_creator_voice_orchestration_v1(
        &mut fixture.store,
        &artifacts,
        &fixture.content,
        &runtime("voice-settings-v1"),
        &[provider()],
    )
    .unwrap();
    assert_eq!(retry.burst.scheduled_job_count(), 2);

    let mut succeeding = FakeExecutor::default();
    let dispatched =
        dispatch_creator_voice_burst_v1(&mut fixture.store, &mut succeeding, &retry).unwrap();
    assert_eq!(dispatched.dispatched.len(), 2);
    assert!(dispatched.failures.is_empty());

    for segment in &retry.segments {
        assert_eq!(
            fixture
                .store
                .list_voice_takes_v1(&segment.job.job_id)
                .unwrap()
                .len(),
            2
        );
    }
    for dispatch in succeeding.dispatched {
        assert!(
            dispatch.output_uri.as_str().contains("take-0002.wav"),
            "{}",
            dispatch.output_uri.as_str()
        );
        assert_eq!(
            dispatch.plugin_payload["timing"]["schema"],
            "omnicreator.voice-timing"
        );
        assert!(dispatch.plugin_payload["timing"]["output_uri"]
            .as_str()
            .unwrap()
            .contains("take-0002.timing.json"));
    }
}

#[test]
fn p3_voice_settings_change_invalidates_only_voice_and_downstream_work() {
    let mut fixture = fixture();
    let artifacts = omnicreator_core::ArtifactStore::new(fixture.workspace.data_root()).unwrap();
    let first = plan_creator_voice_orchestration_v1(
        &mut fixture.store,
        &artifacts,
        &fixture.content,
        &runtime("voice-settings-v1"),
        &[provider()],
    )
    .unwrap();
    let old_hashes = first
        .segments
        .iter()
        .map(|segment| (segment.segment_id.clone(), segment.job.input_hash.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();

    let changed = plan_creator_voice_orchestration_v1(
        &mut fixture.store,
        &artifacts,
        &fixture.content,
        &runtime("voice-settings-v2"),
        &[provider()],
    )
    .unwrap();

    for segment in &changed.segments {
        assert_ne!(
            old_hashes[&segment.segment_id], segment.job.input_hash,
            "{} must get a new hash when voice settings change",
            segment.segment_id
        );
        assert_eq!(segment.step.status, StepStatus::Ready);
    }

    let steps = fixture.store.list_project_steps(&fixture.project_id).unwrap();
    assert_eq!(
        steps
            .iter()
            .find(|step| step.step == CREATOR_STEP_CONTENT_PREPARE_V1)
            .unwrap()
            .status,
        StepStatus::Succeeded
    );
    assert_eq!(
        steps
            .iter()
            .find(|step| step.step == CREATOR_STEP_VOICE_PREPARE_V1)
            .unwrap()
            .status,
        StepStatus::NotReady
    );
    assert_eq!(
        steps
            .iter()
            .find(|step| step.step == CREATOR_STEP_PRODUCTION_PACK_V1)
            .unwrap()
            .status,
        StepStatus::Stale
    );
}
