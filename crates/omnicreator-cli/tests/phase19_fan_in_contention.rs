use std::process::Command;

use omnicreator_application::ApplicationControlService;
use omnicreator_core::{initial_studio_pack_catalog_v1, Workspace, WorkspaceSession};
use serde_json::{json, Value};

fn run(root: &std::path::Path, extra: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_omnicreator"));
    command
        .arg("--data-root")
        .arg(root)
        .arg("--device-id")
        .arg("phase19-p5-cli-contender")
        .arg("--json");
    command.args(extra);
    command.output().unwrap()
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

    let qa = run(&root, &["work", "qa", "--project", project.id.as_str()]);
    assert_eq!(qa.status.code(), Some(0), "{}", String::from_utf8_lossy(&qa.stdout));
    let qa_json: Value = serde_json::from_slice(&qa.stdout).unwrap();
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
    let commit = run(
        &root,
        &["work", "commit", "--input-file", payload.to_str().unwrap()],
    );
    assert_eq!(
        commit.status.code(),
        Some(5),
        "{}",
        String::from_utf8_lossy(&commit.stdout)
    );
    let commit_json: Value = serde_json::from_slice(&commit.stdout).unwrap();
    assert_eq!(commit_json["ok"], false);
    assert_eq!(commit_json["error"]["code"], "writer_conflict");

    drop(holder);
}
