use std::{fs, io::Cursor};

use omnicreator_application::ManualContentRequestV1;
use omnicreator_cli::run_cli_v1;
use omnicreator_core::ManualResultProvenanceV1;
use serde::Serialize;
use serde_json::Value;

fn run(args: Vec<String>, stdin: &str) -> omnicreator_cli::CliRunResultV1 {
    let mut input = Cursor::new(stdin.as_bytes());
    run_cli_v1(args, &mut input)
}

fn args(root: &std::path::Path, command: &[&str]) -> Vec<String> {
    let mut args = vec![
        "--data-root".to_owned(),
        root.to_string_lossy().to_string(),
        "--device-id".to_owned(),
        "phase18-p1-contract-edge".to_owned(),
        "--json".to_owned(),
    ];
    args.extend(command.iter().map(|value| (*value).to_owned()));
    args
}

fn json(result: &omnicreator_cli::CliRunResultV1) -> Value {
    serde_json::from_str(&result.output).unwrap()
}

fn payload<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap()
}

#[test]
fn auto_off_still_accepts_manual_result_and_project_review_accepts_project_filter() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data-root");

    let init = run(args(&root, &["workspace", "init"]), "");
    assert_eq!(init.exit_code, 0, "{}", init.output);

    let created = run(
        args(
            &root,
            &[
                "project",
                "create",
                "--title",
                "Policy and input file acceptance",
                "--studio-pack",
                "christian-cinematic",
            ],
        ),
        "",
    );
    assert_eq!(created.exit_code, 0, "{}", created.output);
    let project_id = json(&created)["data"]["data"]["project"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let automatic = run(
        args(
            &root,
            &[
                "workflow",
                "set-auto",
                "--project",
                &project_id,
                "--step",
                "content.prepare",
                "--off",
            ],
        ),
        "",
    );
    assert_eq!(automatic.exit_code, 0, "{}", automatic.output);
    assert_eq!(
        json(&automatic)["data"]["data"]["automatic_execution_enabled"],
        false
    );

    let request = ManualContentRequestV1 {
        project_id: project_id.clone(),
        script: "Automatic execution is OFF, but canonical manual content is still required."
            .to_owned(),
        provenance: ManualResultProvenanceV1::manual_editor(),
    };
    let request_file = temp.path().join("manual-content.json");
    fs::write(&request_file, payload(&request)).unwrap();
    let provided = run(
        args(
            &root,
            &[
                "content",
                "provide",
                "--input-file",
                request_file.to_str().unwrap(),
            ],
        ),
        "",
    );
    assert_eq!(provided.exit_code, 0, "{}", provided.output);
    assert_eq!(
        json(&provided)["data"]["data"]["content"]["script"],
        request.script
    );

    let policies = run(
        args(&root, &["workflow", "status", "--project", &project_id]),
        "",
    );
    assert_eq!(policies.exit_code, 0, "{}", policies.output);
    let content_policy = json(&policies)["data"]["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|policy| policy["step"] == "content.prepare")
        .unwrap()
        .clone();
    assert_eq!(content_policy["automatic_execution_enabled"], false);

    let review = run(
        args(&root, &["review", "list", "--project", &project_id]),
        "",
    );
    assert_eq!(review.exit_code, 0, "{}", review.output);
    assert_eq!(json(&review)["data"]["operation"], "ReviewCenter");
    assert!(json(&review)["data"]["data"].is_object());
}
