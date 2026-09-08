use std::fs;

use omnicreator_core::{
    compile_creator_workflow_plan_v1, ingest_manual_result_file_v1,
    initial_studio_pack_catalog_v1, list_manual_result_history_v1,
    materialize_creator_workflow_plan_v1, ArtifactStore, LogicalUri,
    ManualResultIngestRequestV1, ManualResultProvenanceV1, StateStore, StepStatus, Workspace,
    CREATOR_STEP_CONTENT_PREPARE_V1, CREATOR_STEP_PRODUCTION_PACK_V1,
    CREATOR_STEP_SCENE_PLAN_V1, CREATOR_STEP_VISUAL_PREPARE_V1,
    CREATOR_STEP_VOICE_PREPARE_V1, CREATOR_WORKFLOW_UNIT_PROJECT_V1, MANUAL_RESULT_SCHEMA_V1,
    MANUAL_RESULT_VERSION_V1,
};

struct Fixture {
    temp: tempfile::TempDir,
    workspace: Workspace,
    store: StateStore,
    project_id: String,
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
        .create_project_with_studio_pack("Manual P0", Some(&pack.id))
        .unwrap();
    let plan = compile_creator_workflow_plan_v1(&project, &pack).unwrap();
    materialize_creator_workflow_plan_v1(&store, &plan).unwrap();
    Fixture {
        temp,
        workspace,
        store,
        project_id: project.id,
    }
}

fn request(
    project_id: &str,
    workflow_step: &str,
    workflow_unit: &str,
    job_step: &str,
    job_unit: &str,
    target: &str,
    replace_existing: bool,
    complete_workflow_step: bool,
) -> ManualResultIngestRequestV1 {
    ManualResultIngestRequestV1 {
        schema: MANUAL_RESULT_SCHEMA_V1.to_owned(),
        version: MANUAL_RESULT_VERSION_V1,
        project_id: project_id.to_owned(),
        workflow_step: workflow_step.to_owned(),
        workflow_unit: workflow_unit.to_owned(),
        job_step: job_step.to_owned(),
        job_unit: job_unit.to_owned(),
        artifact_type: "manual-test".to_owned(),
        target_uri: LogicalUri::parse(target).unwrap(),
        provenance: ManualResultProvenanceV1::local_file(),
        stage_metadata: serde_json::json!({"contract": "p0-test"}),
        replace_existing,
        complete_workflow_step,
    }
}

fn project_step(store: &StateStore, project_id: &str, key: &str) -> omnicreator_core::WorkflowStep {
    store
        .list_project_steps(project_id)
        .unwrap()
        .into_iter()
        .find(|step| step.step == key && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1)
        .unwrap()
}

#[test]
fn p0_manual_ingest_is_canonical_portable_and_read_only_inspectable() {
    let mut fx = fixture();
    let source = fx.temp.path().join("creator-script.txt");
    fs::write(&source, b"Creator authored script").unwrap();
    let req = request(
        &fx.project_id,
        CREATOR_STEP_CONTENT_PREPARE_V1,
        CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        CREATOR_STEP_CONTENT_PREPARE_V1,
        CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        "project://manual-results/content/result.txt",
        false,
        true,
    );

    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();
    let outcome =
        ingest_manual_result_file_v1(&mut fx.store, &artifacts, &req, &source).unwrap();
    assert_eq!(outcome.job.status, StepStatus::Succeeded);
    assert_eq!(outcome.attempt.status, StepStatus::Succeeded);
    assert_eq!(outcome.attempt.worker.as_deref(), Some("manual-result:local-file"));
    assert!(artifacts.verify_artifact(&outcome.artifact).unwrap());
    assert_eq!(
        project_step(&fx.store, &fx.project_id, CREATOR_STEP_CONTENT_PREPARE_V1).status,
        StepStatus::Succeeded
    );
    let metadata = serde_json::to_string(&outcome.artifact.metadata).unwrap();
    assert!(!metadata.contains(source.to_string_lossy().as_ref()));
    assert!(!metadata.contains(fx.workspace.data_root().to_string_lossy().as_ref()));

    let cached =
        ingest_manual_result_file_v1(&mut fx.store, &artifacts, &req, &source).unwrap();
    assert!(cached.cache_hit);
    assert_eq!(cached.artifact.artifact_id, outcome.artifact.artifact_id);

    let old_root = fx.workspace.data_root().to_path_buf();
    let sqlite = fx.workspace.sqlite_path();
    drop(fx.store);
    drop(artifacts);
    drop(fx.workspace);
    let moved = fx.temp.path().join("moved-data");
    fs::rename(&old_root, &moved).unwrap();

    let reopened = Workspace::open(&moved).unwrap();
    let read_only = StateStore::open_read_only(reopened.sqlite_path()).unwrap();
    let moved_artifacts = ArtifactStore::new(reopened.data_root()).unwrap();
    let history = list_manual_result_history_v1(
        &read_only,
        &moved_artifacts,
        &fx.project_id,
        CREATOR_STEP_CONTENT_PREPARE_V1,
        CREATOR_WORKFLOW_UNIT_PROJECT_V1,
    )
    .unwrap();
    assert_eq!(history.len(), 1);
    assert!(history[0].verified);
    assert_eq!(
        history[0]
            .provenance
            .as_ref()
            .unwrap()
            .producer,
        omnicreator_core::ManualResultProducerV1::LocalFileImport
    );

    let mut read_only = read_only;
    let readonly_source = fx.temp.path().join("readonly.txt");
    fs::write(&readonly_source, b"replacement").unwrap();
    let mut readonly_req = req.clone();
    readonly_req.target_uri =
        LogicalUri::parse("project://manual-results/content/readonly.txt").unwrap();
    readonly_req.replace_existing = true;
    assert!(ingest_manual_result_file_v1(
        &mut read_only,
        &moved_artifacts,
        &readonly_req,
        &readonly_source
    )
    .is_err());
    assert!(!sqlite.exists());
}

#[test]
fn p0_replacement_invalidates_only_owner_dependency_cone_and_preserves_history() {
    let mut fx = fixture();
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();

    for key in [
        CREATOR_STEP_CONTENT_PREPARE_V1,
        CREATOR_STEP_SCENE_PLAN_V1,
    ] {
        let step = project_step(&fx.store, &fx.project_id, key);
        fx.store.set_step_status(&step.step_id, StepStatus::Succeeded).unwrap();
        fx.store.refresh_ready_steps(&fx.project_id).unwrap();
    }

    let visual_a = fx.temp.path().join("visual-a.png");
    fs::write(&visual_a, b"visual A").unwrap();
    let first = request(
        &fx.project_id,
        CREATOR_STEP_VISUAL_PREPARE_V1,
        CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        CREATOR_STEP_VISUAL_PREPARE_V1,
        "SC001",
        "project://manual-results/visual/SC001-a.png",
        false,
        false,
    );
    let first_outcome =
        ingest_manual_result_file_v1(&mut fx.store, &artifacts, &first, &visual_a).unwrap();
    assert_eq!(first_outcome.job.status, StepStatus::Succeeded);

    let visual_step = project_step(&fx.store, &fx.project_id, CREATOR_STEP_VISUAL_PREPARE_V1);
    let voice_step = project_step(&fx.store, &fx.project_id, CREATOR_STEP_VOICE_PREPARE_V1);
    fx.store
        .set_step_status(&visual_step.step_id, StepStatus::Succeeded)
        .unwrap();
    fx.store
        .set_step_status(&voice_step.step_id, StepStatus::Succeeded)
        .unwrap();
    fx.store.refresh_ready_steps(&fx.project_id).unwrap();
    let production =
        project_step(&fx.store, &fx.project_id, CREATOR_STEP_PRODUCTION_PACK_V1);
    fx.store
        .set_step_status(&production.step_id, StepStatus::Succeeded)
        .unwrap();

    let visual_b = fx.temp.path().join("visual-b.png");
    fs::write(&visual_b, b"visual B").unwrap();
    let second = request(
        &fx.project_id,
        CREATOR_STEP_VISUAL_PREPARE_V1,
        CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        CREATOR_STEP_VISUAL_PREPARE_V1,
        "SC001",
        "project://manual-results/visual/SC001-b.png",
        true,
        false,
    );
    let replaced =
        ingest_manual_result_file_v1(&mut fx.store, &artifacts, &second, &visual_b).unwrap();

    assert!(replaced
        .invalidated_step_ids
        .iter()
        .any(|id| id == &visual_step.step_id));
    assert!(replaced
        .invalidated_step_ids
        .iter()
        .any(|id| id == &production.step_id));
    assert_eq!(
        project_step(&fx.store, &fx.project_id, CREATOR_STEP_VISUAL_PREPARE_V1).status,
        StepStatus::Ready
    );
    assert_eq!(
        project_step(&fx.store, &fx.project_id, CREATOR_STEP_PRODUCTION_PACK_V1).status,
        StepStatus::NotReady
    );
    assert_eq!(
        project_step(&fx.store, &fx.project_id, CREATOR_STEP_VOICE_PREPARE_V1).status,
        StepStatus::Succeeded
    );

    let jobs = fx.store.list_project_jobs(&fx.project_id).unwrap();
    let old = jobs
        .iter()
        .find(|job| job.job_id == first_outcome.job.job_id)
        .unwrap();
    assert_eq!(old.status, StepStatus::Stale);
    let new = jobs
        .iter()
        .find(|job| job.job_id == replaced.job.job_id)
        .unwrap();
    assert_eq!(new.status, StepStatus::Succeeded);

    let history = list_manual_result_history_v1(
        &fx.store,
        &artifacts,
        &fx.project_id,
        CREATOR_STEP_VISUAL_PREPARE_V1,
        "SC001",
    )
    .unwrap();
    assert_eq!(history.len(), 2);
    assert!(history.iter().all(|entry| entry.verified));
}

#[test]
fn p0_rejects_machine_paths_and_secret_shaped_metadata_before_attempt() {
    let mut fx = fixture();
    let source = fx.temp.path().join("input.txt");
    fs::write(&source, b"value").unwrap();
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();
    let mut req = request(
        &fx.project_id,
        CREATOR_STEP_CONTENT_PREPARE_V1,
        CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        CREATOR_STEP_CONTENT_PREPARE_V1,
        CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        "project://manual-results/content/rejected.txt",
        false,
        true,
    );
    req.stage_metadata = serde_json::json!({"api_token": "secret-value"});
    assert!(ingest_manual_result_file_v1(&mut fx.store, &artifacts, &req, &source).is_err());

    req.stage_metadata = serde_json::json!({"safe": "C:\\Users\\creator\\input.txt"});
    assert!(ingest_manual_result_file_v1(&mut fx.store, &artifacts, &req, &source).is_err());

    assert!(fx.store.list_project_jobs(&fx.project_id).unwrap().is_empty());
}
