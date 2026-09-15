use std::io::Cursor;

use omnicreator_application::ApplicationControlService;
use omnicreator_cli::run_cli_v1;
use omnicreator_core::{initial_studio_pack_catalog_v1, Workspace, WorkspaceSession};
use serde_json::{json, Value};

fn args(root: &std::path::Path, extra: &[&str]) -> Vec<String> {
    let mut args = vec![
        "--data-root".to_owned(),
        root.to_string_lossy().to_string(),
        "--device-id".to_owned(),
        "phase19-p5-cli-contender".to_owned(),
        "--json".to_owned(),
    ];
    args.extend(extra.iter().map(|value| (*value).to_owned()));
    args
}

#[test]
fn fan_in_qa_remains_readable_while_canonical_commit_respects_writer_lease() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data-root");
    let workspace = Workspace::create(&root).unwrap();
    let seed_session = WorkspaceSession::acquire(workspace, "phase19-p5-seed").unwrap();
    let mut service = ApplicationControlService::for_writer(&seed_session).unwrap();
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let project = service
        .create_creator_project_v1("Phase 19 P5 contention", &pack)
        .unwrap()
        .data
        .project;
    drop(service);
    seed_session.release().unwrap();

    let workspace = Workspace::open(&root).unwrap();
    let holder = WorkspaceSession::acquire(workspace, "phase19-p5-active-writer").unwrap();

    let mut stdin = Cursor::new(Vec::<u8>::new());
    let qa = run_cli_v1(
        args(
            &root,
            &["work", "qa", "--project", project.id.as_str()],
        ),
        &mut stdin,
    );
    assert_eq!(qa.exit_code, 0, "{}", qa.output);
    let qa_json: Value = serde_json::from_str(&qa.output).unwrap();
    assert_eq!(qa_json["ok"], true);
    assert_eq!(qa_json["operation"], "work.qa");
    assert_eq!(qa_json["data"]["read_only"], true);

    let payload = temp.path().join("empty-batch.json");
    std::fs::write(
        &payload,
        serde_json::to_vec(&json!({
            "schema": "omnicreator.external-result-batch",
            "version": 1,
            "items": []
        }))
        .unwrap(),
    )
    .unwrap();
    let mut stdin = Cursor::new(Vec::<u8>::new());
    let commit = run_cli_v1(
        args(
            &root,
            &[
                "work",
                "commit",
                "--input-file",
                payload.to_str().unwrap(),
            ],
        ),
        &mut stdin,
    );
    assert_eq!(commit.exit_code, 5, "{}", commit.output);
    let commit_json: Value = serde_json::from_str(&commit.output).unwrap();
    assert_eq!(commit_json["ok"], false);
    assert_eq!(commit_json["error"]["code"], "writer_conflict");

    drop(holder);
}
