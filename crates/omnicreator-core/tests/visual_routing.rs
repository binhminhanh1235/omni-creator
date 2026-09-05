use std::{collections::BTreeMap, fs, path::PathBuf};

use omnicreator_core::{
    attach_visual_routing_to_promotion_v1, execute_generated_image_plugin_with_options_v1,
    route_scene_visual_v1, route_thumbnail_background_v1, scan_plugin_roots, ArtifactStore,
    GeneratedImageExecutionContextV1, GeneratedImageExecutionOptionsV1,
    GeneratedImagePreparationV1, GeneratedImageRequestV1,
    GeneratedImageResolutionV1, GeneratedImageStyleV1, LogicalUri, PluginJobWorkspace,
    PluginProcessOptions, RankedVisualCandidate, SceneIntentV1, SelectedVisualOutput, StateStore,
    StepStatus, StockDiscoveryStatusV1, VisualCandidate, VisualCandidatePreview,
    VisualCandidateScore, VisualMediaType, VisualPreviewKind, VisualRouteV1, VisualRoutingPolicyV1,
    VisualRoutingReasonV1, VisualUseCaseV1, Workspace, SCENE_INTENT_SCHEMA,
    SCENE_INTENT_SCHEMA_VERSION,
};

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
        id: "SC17".to_owned(),
        segment_id: "S04".to_owned(),
        narration: "Forgiveness does not automatically restore trust.".to_owned(),
        purpose: "Show cautious rebuilding of trust.".to_owned(),
        scene_type: "conceptual".to_owned(),
        emotion_before: Some("uncertainty".to_owned()),
        emotion_after: Some("cautious hope".to_owned()),
        duration_hint: Some(11.5),
        visual_ideas: vec!["repairing a bridge".to_owned()],
        search_queries: vec!["rebuilding fence careful hands".to_owned()],
        avoid: vec!["generic praying hands".to_owned()],
        continuity: BTreeMap::new(),
        aspect_ratio: "16:9".to_owned(),
    }
}

fn ranked(id: &str, score: f64) -> RankedVisualCandidate {
    RankedVisualCandidate {
        candidate: VisualCandidate {
            candidate_id: id.to_owned(),
            scene_id: "SC17".to_owned(),
            source_provider: "pexels".to_owned(),
            source_asset_id: format!("asset-{id}"),
            selection_ref: format!("pexels:image:{id}"),
            media_type: VisualMediaType::Image,
            title: Some(format!("Candidate {id}")),
            description: None,
            tags: vec!["repair".to_owned()],
            source_page_url: Some(format!("https://www.pexels.com/photo/{id}/")),
            creator_name: Some("Fixture Creator".to_owned()),
            creator_url: None,
            width: Some(1920),
            height: Some(1080),
            duration: None,
            previews: vec![VisualCandidatePreview {
                kind: VisualPreviewKind::Thumbnail,
                url: format!("https://preview.example/{id}.jpg"),
                width: Some(640),
                height: Some(360),
                duration: None,
            }],
        },
        score: VisualCandidateScore {
            semantic_relevance: 0.30,
            emotional_relevance: 0.15,
            narrative_purpose: 0.12,
            visual_quality: 0.08,
            channel_continuity: 0.07,
            editability: 0.04,
            freshness: 0.04,
            content_match_score: 0.57,
            base_score: score,
            cliche_matches: Vec::new(),
            cliche_penalty: 0.0,
            reuse_penalty: 0.0,
            final_score: score,
        },
    }
}

fn generated_request() -> GeneratedImageRequestV1 {
    GeneratedImageRequestV1::from_scene_v1(
        scene(),
        GeneratedImageStyleV1 {
            preset: "cinematic-warm".to_owned(),
            description: Some("natural texture, restrained contrast".to_owned()),
        },
        GeneratedImageResolutionV1 {
            width: 1280,
            height: 720,
        },
        Some(42),
        BTreeMap::new(),
    )
    .unwrap()
}

fn preparation(output_uri: &str) -> GeneratedImagePreparationV1 {
    GeneratedImagePreparationV1 {
        request: generated_request(),
        output_uri: Some(LogicalUri::parse(output_uri).unwrap()),
        provider_id: None,
        model_id: Some("reference-svg".to_owned()),
        model_version: Some("1".to_owned()),
        approval_required: false,
        approval_complete: true,
        production_lock_required: false,
        gpu_execution_requested: false,
    }
}

#[test]
fn viable_stock_stays_preview_first_and_routing_provenance_commits_with_artifact() {
    let routing = route_scene_visual_v1(
        &scene(),
        StockDiscoveryStatusV1::Complete,
        &[ranked("A", 0.92)],
        VisualRoutingPolicyV1 {
            minimum_stock_score: 0.80,
        },
    )
    .unwrap();
    assert_eq!(routing.route, VisualRouteV1::StockReview);
    assert!(routing.preview_first);

    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Stock Route").unwrap();
    let job = state
        .create_job(&project.id, "visual", "SC17", "stock-route-input")
        .unwrap();

    let plugin_workspace =
        PluginJobWorkspace::create(temp.path().join("plugin-runtime"), &job.job_id).unwrap();
    let relative_output = "selected/stock-image.jpg";
    let output = plugin_workspace.resolve_output(relative_output).unwrap();
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(&output, b"preview-first selected stock fixture").unwrap();

    let selected = SelectedVisualOutput {
        source_provider: "pexels".to_owned(),
        source_asset_id: "asset-A".to_owned(),
        selection_ref: "pexels:image:A".to_owned(),
        media_type: VisualMediaType::Image,
        relative_output: relative_output.to_owned(),
        width: Some(1920),
        height: Some(1080),
        duration: None,
        provenance: BTreeMap::new(),
    };
    let promotion = attach_visual_routing_to_promotion_v1(
        selected
            .promotion(LogicalUri::parse("project://visual/SC17.jpg").unwrap())
            .unwrap(),
        &routing,
    )
    .unwrap();

    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let artifact = artifacts
        .promote_plugin_output(&mut state, &job.job_id, &plugin_workspace, promotion)
        .unwrap();

    assert_eq!(
        state.get_job(&job.job_id).unwrap().status,
        StepStatus::Succeeded
    );
    assert_eq!(artifact.metadata["visual_routing"]["route"], "stock_review");
    assert_eq!(
        artifact.metadata["visual_routing"]["reason"],
        "stock_meets_quality_threshold"
    );
    assert_eq!(artifact.metadata["visual_routing"]["preview_first"], true);
}

#[test]
fn below_threshold_stock_fallback_is_persisted_by_generated_core_path() {
    let routing = route_scene_visual_v1(
        &scene(),
        StockDiscoveryStatusV1::Complete,
        &[ranked("A", 0.72)],
        VisualRoutingPolicyV1 {
            minimum_stock_score: 0.80,
        },
    )
    .unwrap();
    assert_eq!(routing.route, VisualRouteV1::GeneratedStill);

    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Generated Fallback").unwrap();
    let preparation = preparation("project://visual/SC17.svg");
    let job = state
        .create_job(
            &project.id,
            "visual.generate",
            "SC17",
            &preparation.request.input_hash_v1().unwrap(),
        )
        .unwrap();

    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let execution = execute_generated_image_plugin_with_options_v1(
        &mut state,
        &artifacts,
        &generated_plugin(),
        temp.path().join("plugin-runtime"),
        &job.job_id,
        &preparation,
        GeneratedImageExecutionOptionsV1 {
            context: GeneratedImageExecutionContextV1 {
                use_case: VisualUseCaseV1::SceneVisual,
                routing: Some(routing),
            },
            process: PluginProcessOptions::default(),
        },
    )
    .unwrap();

    assert_eq!(
        state.get_job(&job.job_id).unwrap().status,
        StepStatus::Succeeded
    );
    assert_eq!(execution.artifact.metadata["use_case"], "scene_visual");
    assert_eq!(
        execution.artifact.metadata["visual_routing"]["route"],
        "generated_still"
    );
    assert_eq!(
        execution.artifact.metadata["visual_routing"]["reason"],
        "stock_below_quality_threshold"
    );
    assert_eq!(
        execution.artifact.metadata["visual_routing"]["stock_score"],
        0.72
    );
    assert_eq!(
        execution.artifact.metadata["visual_routing"]["minimum_stock_score"],
        0.80
    );
}

#[test]
fn thumbnail_background_uses_same_generated_job_attempt_artifact_model() {
    let routing = route_thumbnail_background_v1(&scene()).unwrap();
    assert_eq!(
        routing.reason,
        VisualRoutingReasonV1::ThumbnailBackgroundRequested
    );

    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Thumbnail Background").unwrap();
    let preparation = preparation("project://thumbnail/SC17-background.svg");
    let job = state
        .create_job(
            &project.id,
            "visual.generate",
            "SC17",
            &preparation.request.input_hash_v1().unwrap(),
        )
        .unwrap();

    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let execution = execute_generated_image_plugin_with_options_v1(
        &mut state,
        &artifacts,
        &generated_plugin(),
        temp.path().join("plugin-runtime"),
        &job.job_id,
        &preparation,
        GeneratedImageExecutionOptionsV1 {
            context: GeneratedImageExecutionContextV1 {
                use_case: VisualUseCaseV1::ThumbnailBackground,
                routing: Some(routing),
            },
            process: PluginProcessOptions::default(),
        },
    )
    .unwrap();

    let attempts = state.list_attempts(&job.job_id).unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].status, StepStatus::Succeeded);
    assert_eq!(
        execution.artifact.metadata["use_case"],
        "thumbnail_background"
    );
    assert_eq!(
        execution.artifact.metadata["visual_routing"]["reason"],
        "thumbnail_background_requested"
    );
    assert!(artifacts.verify_artifact(&execution.artifact).unwrap());
}
