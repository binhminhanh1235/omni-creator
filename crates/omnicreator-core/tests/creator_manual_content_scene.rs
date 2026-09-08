use std::fs;

use omnicreator_core::{
    compile_creator_workflow_plan_v1, import_manual_creator_scene_plan_file_v1,
    import_manual_creator_script_file_v1, initial_studio_pack_catalog_v1,
    load_latest_creator_content_scene_v1, materialize_creator_workflow_plan_v1,
    provide_manual_creator_content_v1, provide_manual_creator_scene_plan_v1, ArtifactStore,
    CreatorScenePlanV1, ManualResultProvenanceV1, ManualScenePlanDraftV1, StateStore, StepStatus,
    Workspace, CREATOR_STEP_CONTENT_PREPARE_V1, CREATOR_STEP_SCENE_PLAN_V1,
    CREATOR_STEP_VISUAL_PREPARE_V1, CREATOR_WORKFLOW_UNIT_PROJECT_V1,
};

fn fixture() -> (tempfile::TempDir, Workspace, StateStore, String) {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let store = StateStore::open(workspace.sqlite_path()).unwrap();
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let project = store
        .create_project_with_studio_pack("Manual content scene", Some(&pack.id))
        .unwrap();
    let plan = compile_creator_workflow_plan_v1(&project, &pack).unwrap();
    materialize_creator_workflow_plan_v1(&store, &plan).unwrap();
    (temp, workspace, store, project.id)
}

fn step_status(store: &StateStore, project_id: &str, step: &str) -> StepStatus {
    store
        .list_project_steps(project_id)
        .unwrap()
        .into_iter()
        .find(|candidate| {
            candidate.step == step && candidate.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1
        })
        .unwrap()
        .status
}

#[test]
fn no_llm_manual_script_and_scene_plan_unlock_visual() {
    let (_temp, workspace, mut store, project_id) = fixture();
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();

    let content = provide_manual_creator_content_v1(
        &mut store,
        &artifacts,
        &project_id,
        "First narration beat.\n\nSecond narration beat.",
        ManualResultProvenanceV1::manual_editor(),
    )
    .unwrap();
    assert_eq!(content.content.segments.len(), 2);
    assert_eq!(content.content.segments[0].id, "S001");
    assert_eq!(
        step_status(&store, &project_id, CREATOR_STEP_CONTENT_PREPARE_V1),
        StepStatus::Succeeded
    );
    assert_eq!(
        step_status(&store, &project_id, CREATOR_STEP_SCENE_PLAN_V1),
        StepStatus::Ready
    );

    let mut draft = ManualScenePlanDraftV1::for_content_v1(&content.content).unwrap();
    draft.scenes[0].purpose = "Establish the opening".to_owned();
    draft.scenes[0].visual_ideas = vec!["sunrise over a quiet city".to_owned()];
    draft.scenes[1].scene_type = "illustration".to_owned();
    let scene = provide_manual_creator_scene_plan_v1(
        &mut store,
        &artifacts,
        &project_id,
        &draft,
        ManualResultProvenanceV1::manual_editor(),
    )
    .unwrap();

    assert_eq!(scene.scene_plan.scenes[0].id, "SC001");
    assert_eq!(scene.scene_plan.scenes[0].segment_id, "S001");
    assert_eq!(
        scene.scene_plan.scenes[0].narration,
        content.content.segments[0].text
    );
    assert_eq!(
        scene.scene_plan.content_sha256,
        content.artifact.sha256
    );
    assert_eq!(
        step_status(&store, &project_id, CREATOR_STEP_SCENE_PLAN_V1),
        StepStatus::Succeeded
    );
    assert_eq!(
        step_status(&store, &project_id, CREATOR_STEP_VISUAL_PREPARE_V1),
        StepStatus::Ready
    );

    let loaded =
        load_latest_creator_content_scene_v1(&store, &artifacts, &project_id).unwrap();
    assert!(loaded.is_some());
}

#[test]
fn txt_markdown_and_portable_scene_import_normalize_identity() {
    let (temp, workspace, mut store, project_id) = fixture();
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let script = temp.path().join("script.md");
    fs::write(&script, "# ignored as narration text\n\nSecond beat.").unwrap();
    let content =
        import_manual_creator_script_file_v1(&mut store, &artifacts, &project_id, &script)
            .unwrap();

    let mut imported = CreatorScenePlanV1 {
        schema: "omnicreator.creator-scene-plan".to_owned(),
        schema_version: 1,
        project_id: "different-project".to_owned(),
        content_sha256: "0".repeat(64),
        scenes: content
            .content
            .segments
            .iter()
            .enumerate()
            .map(|(index, segment)| omnicreator_core::SceneIntentV1 {
                schema: omnicreator_core::SCENE_INTENT_SCHEMA.to_owned(),
                schema_version: omnicreator_core::SCENE_INTENT_SCHEMA_VERSION,
                id: format!("FOREIGN-{index}"),
                segment_id: "FOREIGN".to_owned(),
                narration: "foreign narration".to_owned(),
                purpose: format!("Purpose {}", index + 1),
                scene_type: "b-roll".to_owned(),
                emotion_before: None,
                emotion_after: None,
                duration_hint: Some(3.0),
                visual_ideas: vec![segment.text.clone()],
                search_queries: Vec::new(),
                avoid: Vec::new(),
                continuity: Default::default(),
                aspect_ratio: "16:9".to_owned(),
            })
            .collect(),
    };
    imported.scenes[0].id = "SC999".to_owned();
    let path = temp.path().join("portable-scene-plan.json");
    fs::write(&path, serde_json::to_vec(&imported).unwrap()).unwrap();

    let outcome =
        import_manual_creator_scene_plan_file_v1(&mut store, &artifacts, &project_id, &path)
            .unwrap();
    assert_eq!(outcome.scene_plan.project_id, project_id);
    assert_eq!(
        outcome.scene_plan.content_sha256,
        content.artifact.sha256
    );
    for (index, scene) in outcome.scene_plan.scenes.iter().enumerate() {
        assert_eq!(scene.id, format!("SC{:03}", index + 1));
        assert_eq!(scene.segment_id, content.content.segments[index].id);
        assert_eq!(scene.narration, content.content.segments[index].text);
    }
}

#[test]
fn replacing_manual_script_invalidates_scene_and_visual_but_reuses_same_input() {
    let (_temp, workspace, mut store, project_id) = fixture();
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let first = provide_manual_creator_content_v1(
        &mut store,
        &artifacts,
        &project_id,
        "Original beat.",
        ManualResultProvenanceV1::manual_editor(),
    )
    .unwrap();
    let draft = ManualScenePlanDraftV1::for_content_v1(&first.content).unwrap();
    provide_manual_creator_scene_plan_v1(
        &mut store,
        &artifacts,
        &project_id,
        &draft,
        ManualResultProvenanceV1::manual_editor(),
    )
    .unwrap();

    let same = provide_manual_creator_content_v1(
        &mut store,
        &artifacts,
        &project_id,
        "Original beat.",
        ManualResultProvenanceV1::manual_editor(),
    )
    .unwrap();
    assert!(same.ingestion.cache_hit);
    assert_eq!(same.artifact.artifact_id, first.artifact.artifact_id);

    let changed = provide_manual_creator_content_v1(
        &mut store,
        &artifacts,
        &project_id,
        "Replacement beat.",
        ManualResultProvenanceV1::manual_editor(),
    )
    .unwrap();
    assert!(!changed.ingestion.cache_hit);
    assert_eq!(
        step_status(&store, &project_id, CREATOR_STEP_CONTENT_PREPARE_V1),
        StepStatus::Succeeded
    );
    assert_eq!(
        step_status(&store, &project_id, CREATOR_STEP_SCENE_PLAN_V1),
        StepStatus::Ready
    );
    assert_eq!(
        step_status(&store, &project_id, CREATOR_STEP_VISUAL_PREPARE_V1),
        StepStatus::NotReady
    );
}
