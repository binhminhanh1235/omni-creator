use std::{cell::Cell, fs};

use chrono::{TimeZone, Utc};
use omnicreator_core::artifact_store::{AttemptOutputPromotion, AttemptPromotionRequest};
use omnicreator_core::{
    assemble_creator_production_pack_v1, compile_creator_workflow_plan_v1,
    default_segment_tts_compute_requirements_v1, execute_creator_visual_plan_v1,
    initial_studio_pack_catalog_v1, materialize_creator_workflow_plan_v1,
    plan_creator_visuals_v1, plan_creator_voice_orchestration_v1, run_creator_content_scene_v1,
    Artifact, ArtifactStore, ComputeDeviceV1, ComputeProviderCapabilitiesV1,
    ComputeProviderConnectionState, ComputeProviderSchedulingSnapshotV1,
    ComputeProviderSessionIdentityV1, ComputeProviderSessionV1, CreatorContentSceneOptionsV1,
    CreatorInputV1, CreatorLlmExecutorV1, CreatorProductionPackOptionsV1,
    CreatorStockDiscoveryV1, CreatorVisualAssetExecutorV1, CreatorVisualDiscoveryExecutorV1,
    CreatorVisualGenerationRequestV1, CreatorVisualPlanningOptionsV1,
    CreatorVisualStockFetchRequestV1, CreatorVoiceRuntimeV1, Error, LogicalUri, PathResolver,
    ProductionPackageExporterV1, PronunciationRuleV1, Result, SceneIntentV1,
    SegmentTtsLockStateV1, SegmentV1, StateStore, StockDiscoveryStatusV1, StepStatus,
    StudioPackRouteTargetV1, VoiceIdentityV1, VoiceModelIdentityV1, VoiceTimingCueV1,
    VoiceTimingV1, Workspace, CREATOR_STEP_PRODUCTION_PACK_V1, CREATOR_STEP_VISUAL_PREPARE_V1,
    CREATOR_STEP_VOICE_PREPARE_V1, SCENE_INTENT_SCHEMA, SCENE_INTENT_SCHEMA_VERSION,
    VOICE_AUDIO_ARTIFACT_TYPE_V1, VOICE_TIMING_ARTIFACT_TYPE_V1, VOICE_TIMING_SCHEMA_V1,
};
use sha2::{Digest, Sha256};

struct OfflineLlm;

impl CreatorLlmExecutorV1 for OfflineLlm {
    fn create_script_v1(&self, _input: &CreatorInputV1) -> Result<String> {
        Ok(
            "Trust grows through patient repair.\n\nSmall faithful steps make restoration visible."
                .to_owned(),
        )
    }

    fn create_scene_intent_v1(
        &self,
        segment: &SegmentV1,
        scene_id: &str,
        options: &CreatorContentSceneOptionsV1,
    ) -> Result<SceneIntentV1> {
        Ok(SceneIntentV1 {
            schema: SCENE_INTENT_SCHEMA.to_owned(),
            schema_version: SCENE_INTENT_SCHEMA_VERSION,
            id: scene_id.to_owned(),
            segment_id: segment.id.clone(),
            narration: segment.text.clone(),
            purpose: format!("Visualize {}", segment.id),
            scene_type: "conceptual".to_owned(),
            emotion_before: Some("uncertain".to_owned()),
            emotion_after: Some("hopeful".to_owned()),
            duration_hint: Some(2.0),
            visual_ideas: vec!["simple symbolic restoration".to_owned()],
            search_queries: vec!["restoration symbol".to_owned()],
            avoid: options.avoid.clone(),
            continuity: Default::default(),
            aspect_ratio: options.aspect_ratio.clone(),
        })
    }
}

struct OfflineDiscovery {
    calls: Cell<usize>,
}

impl CreatorVisualDiscoveryExecutorV1 for OfflineDiscovery {
    fn discover_stock_v1(
        &self,
        _scene: &SceneIntentV1,
        _ordered_targets: &[StudioPackRouteTargetV1],
    ) -> Result<CreatorStockDiscoveryV1> {
        self.calls.set(self.calls.get() + 1);
        Ok(CreatorStockDiscoveryV1 {
            status: StockDiscoveryStatusV1::Complete,
            ranking_inputs: Vec::new(),
        })
    }
}

struct OfflineVisualExecutor;

impl OfflineVisualExecutor {
    fn promote(
        &self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        job_id: &str,
        scene: &SceneIntentV1,
        capability: &str,
        routing: &omnicreator_core::VisualRoutingDecisionV1,
    ) -> Result<Artifact> {
        let attempt = state_store.start_attempt(job_id, Some("phase15-e2e-visual"))?;
        let staging_dir = artifact_store.data_root().join("cache").join("phase15-e2e");
        fs::create_dir_all(&staging_dir)?;
        let source = staging_dir.join(format!("{job_id}.svg"));
        fs::write(
            &source,
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720"><text x="20" y="40">{}</text></svg>"#,
                scene.id
            ),
        )?;
        let artifacts = artifact_store.promote_attempt_outputs(
            state_store,
            AttemptPromotionRequest {
                attempt_id: attempt.attempt_id,
                job_id: job_id.to_owned(),
                outputs: vec![AttemptOutputPromotion {
                    source: source.clone(),
                    target_uri: LogicalUri::parse(&format!(
                        "project://visual/{}/{}.svg",
                        scene.id, job_id
                    ))?,
                    artifact_type: "image".to_owned(),
                    metadata: serde_json::json!({
                        "source_provider": "phase15-e2e",
                        "capability": capability,
                        "visual_routing": routing,
                    }),
                    expected_sha256: None,
                }],
                selected_output_index: 0,
            },
        )?;
        let _ = fs::remove_file(source);
        artifacts.into_iter().next().ok_or_else(|| {
            Error::InvalidArtifact("offline visual fixture produced no artifact".to_owned())
        })
    }
}

impl CreatorVisualAssetExecutorV1 for OfflineVisualExecutor {
    fn fetch_selected_stock_v1(
        &self,
        _state_store: &mut StateStore,
        _artifact_store: &ArtifactStore,
        _request: CreatorVisualStockFetchRequestV1<'_>,
    ) -> Result<Artifact> {
        Err(Error::InvalidContract(
            "offline Phase 15 fixture must not use stock fetch".to_owned(),
        ))
    }

    fn generate_visual_v1(
        &self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        request: CreatorVisualGenerationRequestV1<'_>,
    ) -> Result<Artifact> {
        self.promote(
            state_store,
            artifact_store,
            request.job_id,
            request.scene,
            &request.target.capability,
            request.routing,
        )
    }
}

fn voice_runtime() -> CreatorVoiceRuntimeV1 {
    CreatorVoiceRuntimeV1 {
        plugin_id: "omnivoice".to_owned(),
        provider_id: "kaggle".to_owned(),
        voice: VoiceIdentityV1 {
            voice_id: "warm-narrator".to_owned(),
            voice_version: "1".to_owned(),
        },
        model: VoiceModelIdentityV1 {
            model_id: "omnivoice-v3".to_owned(),
            model_version: "3.2".to_owned(),
        },
        settings_fingerprint: "phase15-e2e-settings-v1".to_owned(),
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
    let now = Utc.with_ymd_and_hms(2026, 9, 6, 12, 0, 0).unwrap();
    ComputeProviderSchedulingSnapshotV1 {
        state: ComputeProviderConnectionState::Ready,
        session: ComputeProviderSessionV1 {
            identity: ComputeProviderSessionIdentityV1 {
                provider_id: "kaggle".to_owned(),
                session_id: "phase15-e2e-session".to_owned(),
            },
            connected_at: now,
            last_heartbeat_at: now,
            last_healthy_heartbeat_at: Some(now),
            capabilities: ComputeProviderCapabilitiesV1 {
                schema: "omnicreator.compute-capabilities".to_owned(),
                schema_version: 1,
                provider_id: "kaggle".to_owned(),
                devices: vec![ComputeDeviceV1 {
                    id: "gpu0".to_owned(),
                    device_type: "gpu".to_owned(),
                    model: Some("NVIDIA T4".to_owned()),
                    memory_mb: Some(15_360),
                }],
                model_groups: vec!["omnivoice".to_owned()],
                max_parallel_jobs: Some(2),
            },
        },
    }
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn commit_voice_bundle(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    job_id: &str,
    segment_id: &str,
    text: &str,
    duration_ms: u64,
) -> Result<()> {
    let job = state_store.get_job(job_id)?;
    let started = state_store.start_voice_take_attempt_v1(job_id, Some("phase15-e2e-voice"))?;
    let audio_uri = LogicalUri::parse(&format!(
        "project://audio/{segment_id}/take-{:04}.wav",
        started.take_index
    ))?;
    let timing_uri = LogicalUri::parse(&format!(
        "project://audio/{segment_id}/take-{:04}.timing.json",
        started.take_index
    ))?;
    let resolver = PathResolver::new(artifact_store.data_root())?;
    let audio_path = resolver.resolve(&audio_uri, Some(&job.project_id))?;
    let timing_path = resolver.resolve(&timing_uri, Some(&job.project_id))?;
    if let Some(parent) = audio_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let audio_bytes = format!("offline narration for {segment_id}").into_bytes();
    fs::write(&audio_path, &audio_bytes)?;
    let timing = VoiceTimingV1 {
        schema: VOICE_TIMING_SCHEMA_V1.to_owned(),
        version: 1,
        segment_id: segment_id.to_owned(),
        duration_ms,
        cues: vec![VoiceTimingCueV1 {
            index: 0,
            text: text.to_owned(),
            start_ms: 0,
            end_ms: duration_ms,
        }],
    };
    let timing_bytes = timing.to_json_bytes_v1()?;
    fs::write(&timing_path, &timing_bytes)?;

    let audio = Artifact {
        artifact_id: format!("audio-{}", started.attempt.attempt_id),
        project_id: Some(job.project_id.clone()),
        artifact_type: VOICE_AUDIO_ARTIFACT_TYPE_V1.to_owned(),
        uri: audio_uri,
        sha256: sha256(&audio_bytes),
        size_bytes: audio_bytes.len() as u64,
        input_hash: Some(job.input_hash.clone()),
        producer_job: Some(job.job_id.clone()),
        created_at: Utc::now(),
        metadata: serde_json::json!({"fixture": "phase15-e2e"}),
    };
    let timing_artifact = Artifact {
        artifact_id: format!("timing-{}", started.attempt.attempt_id),
        project_id: Some(job.project_id.clone()),
        artifact_type: VOICE_TIMING_ARTIFACT_TYPE_V1.to_owned(),
        uri: timing_uri,
        sha256: sha256(&timing_bytes),
        size_bytes: timing_bytes.len() as u64,
        input_hash: Some(job.input_hash),
        producer_job: Some(job.job_id),
        created_at: Utc::now(),
        metadata: serde_json::json!({"fixture": "phase15-e2e"}),
    };
    state_store.commit_remote_voice_bundle_success_v1(
        &started.attempt.attempt_id,
        &audio,
        &timing_artifact,
    )?;
    Ok(())
}

#[test]
fn phase15_offline_creator_input_reaches_verified_resolve_package() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut store = StateStore::open(workspace.sqlite_path()).unwrap();
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();

    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-stick-explainer")
        .unwrap();
    let project = store
        .create_project_with_studio_pack("Phase 15 E2E", Some(&pack.id))
        .unwrap();
    let workflow = compile_creator_workflow_plan_v1(&project, &pack).unwrap();
    materialize_creator_workflow_plan_v1(&store, &workflow).unwrap();

    let creator = run_creator_content_scene_v1(
        &mut store,
        &artifacts,
        &OfflineLlm,
        &project.id,
        &CreatorInputV1::topic("How trust is rebuilt"),
        &CreatorContentSceneOptionsV1::default(),
    )
    .unwrap();
    assert_eq!(creator.content.segments.len(), 2);
    assert_eq!(creator.scene_plan.scenes.len(), 2);

    let discovery = OfflineDiscovery {
        calls: Cell::new(0),
    };
    let visual_plan = plan_creator_visuals_v1(
        &project,
        &pack,
        &creator.content,
        &creator.scene_plan,
        &creator.scene_plan_artifact.sha256,
        &discovery,
        &CreatorVisualPlanningOptionsV1::default(),
    )
    .unwrap();
    let visual = execute_creator_visual_plan_v1(
        &mut store,
        &artifacts,
        &visual_plan,
        &creator.scene_plan,
        &OfflineVisualExecutor,
    )
    .unwrap();
    assert!(visual.completed);
    assert_eq!(discovery.calls.get(), 0);
    assert_eq!(
        store
            .list_project_steps(&project.id)
            .unwrap()
            .into_iter()
            .find(|step| step.step == CREATOR_STEP_VISUAL_PREPARE_V1)
            .unwrap()
            .status,
        StepStatus::Succeeded
    );

    let voice_plan = plan_creator_voice_orchestration_v1(
        &mut store,
        &artifacts,
        &creator.content,
        &voice_runtime(),
        &[provider()],
    )
    .unwrap();
    assert_eq!(voice_plan.segments.len(), 2);
    assert_eq!(voice_plan.burst.scheduled_job_count(), 2);

    for (index, segment) in voice_plan.segments.iter().enumerate() {
        commit_voice_bundle(
            &mut store,
            &artifacts,
            &segment.job.job_id,
            &segment.segment_id,
            &creator.content.segments[index].text,
            1_000 + (index as u64 * 500),
        )
        .unwrap();
    }

    let completed_voice = plan_creator_voice_orchestration_v1(
        &mut store,
        &artifacts,
        &creator.content,
        &voice_runtime(),
        &[provider()],
    )
    .unwrap();
    assert!(completed_voice.all_complete());
    assert_eq!(
        store
            .list_project_steps(&project.id)
            .unwrap()
            .into_iter()
            .find(|step| step.step == CREATOR_STEP_VOICE_PREPARE_V1)
            .unwrap()
            .status,
        StepStatus::Succeeded
    );

    let assembled = assemble_creator_production_pack_v1(
        &mut store,
        &artifacts,
        &project.id,
        &CreatorProductionPackOptionsV1::default(),
    )
    .unwrap();
    assert_eq!(assembled.total_duration_ms, 2_500);
    assert!(artifacts.verify_artifact(&assembled.artifact).unwrap());
    assert_eq!(
        store
            .list_project_steps(&project.id)
            .unwrap()
            .into_iter()
            .find(|step| step.step == CREATOR_STEP_PRODUCTION_PACK_V1)
            .unwrap()
            .status,
        StepStatus::Succeeded
    );

    let exported = ProductionPackageExporterV1::default()
        .export_v1(&mut store, &artifacts, &assembled.production_pack)
        .unwrap();
    assert_eq!(exported.artifacts.len(), 4);
    assert!(exported
        .artifacts
        .iter()
        .all(|artifact| artifacts.verify_artifact(artifact).unwrap()));
    assert_eq!(
        store.derive_project_status(&project.id).unwrap(),
        omnicreator_core::ProjectDisplayStatus::Done
    );

    let portable = serde_json::to_string(&assembled.production_pack).unwrap();
    assert!(!portable.contains(workspace.data_root().to_string_lossy().as_ref()));
    assert!(portable.contains("project://"));
}
