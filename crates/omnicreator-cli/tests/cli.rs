use std::{fs, io::Cursor};

use omnicreator_application::{
    ApplicationControlService, ManualContentRequestV1, ManualScenePlanRequestV1,
    ManualVisualRequestV1, ManualVoiceRequestV1, ProjectIdRequestV1, StartOrResumeCreatorRequestV1,
};
use omnicreator_cli::run_cli_v1;
use omnicreator_core::{
    derive_manual_voice_timing_v1, CreatorInputV1, ManualResultProvenanceV1,
    ManualScenePlanDraftV1, Workspace,
};
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
        "phase18-p1-cli-test".to_owned(),
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

fn png() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&1280u32.to_be_bytes());
    bytes.extend_from_slice(&720u32.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
    bytes.extend_from_slice(&[0, 0, 0, 0, 1]);
    bytes
}

fn wav(duration_ms: u64) -> Vec<u8> {
    let sample_rate = 8_000u32;
    let channels = 1u16;
    let bits = 16u16;
    let block_align = channels * (bits / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let data_size = ((u64::from(byte_rate) * duration_ms) / 1_000) as u32;
    let mut out = Vec::with_capacity((44 + data_size) as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32 + data_size).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend(std::iter::repeat(7u8).take(data_size as usize));
    out
}

#[test]
fn cli_full_manual_path_reaches_resolve_export_and_matches_application_projection() {
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
                "Phase 18 P1 CLI manual flow",
                "--studio-pack",
                "christian-cinematic",
            ],
        ),
        "",
    );
    assert_eq!(created.exit_code, 0, "{}", created.output);
    let created_json = json(&created);
    let project_id = created_json["data"]["data"]["project"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let script = "The CLI must preserve the same canonical workflow truth as Desktop.";
    let content_request = ManualContentRequestV1 {
        project_id: project_id.clone(),
        script: script.to_owned(),
        provenance: ManualResultProvenanceV1::manual_editor(),
    };
    let content = run(
        args(&root, &["content", "provide", "--stdin"]),
        &payload(&content_request),
    );
    assert_eq!(content.exit_code, 0, "{}", content.output);
    let content_json = json(&content);
    let segment_id = content_json["data"]["data"]["content"]["segments"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let editor = run(
        args(&root, &["scene", "editor", "--project", &project_id]),
        "",
    );
    assert_eq!(editor.exit_code, 0, "{}", editor.output);
    let editor_json = json(&editor);
    let mut draft: ManualScenePlanDraftV1 =
        serde_json::from_value(editor_json["data"][2].clone()).unwrap();
    draft.scenes[0].scene_type = "literal".to_owned();
    draft.scenes[0].purpose = "Prove CLI and Desktop application parity".to_owned();
    let scene_request = ManualScenePlanRequestV1 {
        project_id: project_id.clone(),
        draft,
        provenance: ManualResultProvenanceV1::manual_editor(),
    };
    let scene = run(
        args(&root, &["scene", "provide", "--stdin"]),
        &payload(&scene_request),
    );
    assert_eq!(scene.exit_code, 0, "{}", scene.output);
    let scene_json = json(&scene);
    let scene_id = scene_json["data"]["data"]["scene_plan"]["scenes"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let visual_path = temp.path().join("manual-visual.png");
    fs::write(&visual_path, png()).unwrap();
    let visual_request = ManualVisualRequestV1 {
        project_id: project_id.clone(),
        scene_id,
        source_path: visual_path,
        provenance: ManualResultProvenanceV1::local_file(),
        replace_existing: false,
    };
    let visual = run(
        args(&root, &["visual", "provide", "--stdin"]),
        &payload(&visual_request),
    );
    assert_eq!(visual.exit_code, 0, "{}", visual.output);

    let audio_path = temp.path().join("manual-audio.wav");
    fs::write(&audio_path, wav(1_500)).unwrap();
    let voice_request = ManualVoiceRequestV1 {
        project_id: project_id.clone(),
        segment_id: segment_id.clone(),
        audio_path,
        timing: derive_manual_voice_timing_v1(&segment_id, script, 1_500).unwrap(),
        provenance: ManualResultProvenanceV1::local_file(),
        replace_existing: false,
    };
    let voice = run(
        args(&root, &["voice", "provide", "--stdin"]),
        &payload(&voice_request),
    );
    assert_eq!(voice.exit_code, 0, "{}", voice.output);

    let recovery = run(
        args(&root, &["production", "recovery", "--project", &project_id]),
        "",
    );
    assert_eq!(recovery.exit_code, 0, "{}", recovery.output);
    assert_eq!(
        json(&recovery)["data"]["data"]["recovery"]["ready_for_rebuild"],
        true
    );

    let exported = run(
        args(
            &root,
            &["production", "rebuild-export", "--project", &project_id],
        ),
        "",
    );
    assert_eq!(exported.exit_code, 0, "{}", exported.output);
    let exported_json = json(&exported);
    assert!(
        exported_json["data"]["data"]["assembly"]["production_pack"]["tracks"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert!(exported_json["data"]["data"]["export"]["artifacts"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    let shown = run(
        args(&root, &["project", "show", "--project", &project_id]),
        "",
    );
    assert_eq!(shown.exit_code, 0, "{}", shown.output);
    assert!(!shown.output.contains(root.to_string_lossy().as_ref()));
    assert!(!shown.output.contains("Bearer "));
    assert!(!shown.output.contains("api_key"));

    let workspace = Workspace::inspect(&root).unwrap();
    let service = ApplicationControlService::for_read_only(&workspace).unwrap();
    let direct = service
        .project_status_v1(&ProjectIdRequestV1 {
            project_id: project_id.clone(),
        })
        .unwrap();
    assert_eq!(
        json(&shown)["data"],
        serde_json::to_value(direct).unwrap(),
        "CLI must serialize the exact shared application projection"
    );

    let read_only_rename = run(
        {
            let mut values = args(
                &root,
                &[
                    "project",
                    "rename",
                    "--project",
                    &project_id,
                    "--title",
                    "must not persist",
                ],
            );
            values.push("--read-only".to_owned());
            values
        },
        "",
    );
    assert_eq!(read_only_rename.exit_code, 4);
    assert_eq!(json(&read_only_rename)["error"]["code"], "read_only");
}

#[test]
fn cli_creator_start_reports_provider_unavailable_without_desktop_or_network_attempt() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data-root");
    assert_eq!(run(args(&root, &["workspace", "init"]), "").exit_code, 0);
    let created = run(
        args(
            &root,
            &[
                "project",
                "create",
                "--title",
                "Automatic blocker",
                "--studio-pack",
                "christian-cinematic",
            ],
        ),
        "",
    );
    let project_id = json(&created)["data"]["data"]["project"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let request = StartOrResumeCreatorRequestV1 {
        project_id,
        input: Some(CreatorInputV1::topic(
            "A provider-free deterministic blocker",
        )),
    };
    let result = run(
        args(&root, &["creator", "start", "--stdin"]),
        &payload(&request),
    );
    assert_eq!(result.exit_code, 9, "{}", result.output);
    assert_eq!(json(&result)["error"]["code"], "provider_unavailable");
    assert!(!result.output.contains(root.to_string_lossy().as_ref()));
}
