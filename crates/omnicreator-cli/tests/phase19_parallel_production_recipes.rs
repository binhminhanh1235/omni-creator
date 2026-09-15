use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};

use omnicreator_core::Workspace;
use rmcp::{
    transport::{ConfigureCommandExt, TokioChildProcess},
    ServiceExt,
};
use serde_json::Value;
use tokio::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .to_path_buf()
}

fn read_repo(path: &str) -> String {
    fs::read_to_string(repo_root().join(path))
        .unwrap_or_else(|error| panic!("failed to read repository fixture {path}: {error}"))
}

fn contract() -> Value {
    serde_json::from_str(&read_repo("agent-harness/parallel-production/contract.json")).unwrap()
}

fn mcp_transport(root: &Path) -> TokioChildProcess {
    TokioChildProcess::new(
        Command::new(env!("CARGO_BIN_EXE_omnicreator")).configure(|command| {
            command
                .arg("--data-root")
                .arg(root)
                .arg("--device-id")
                .arg("phase19-p4-parallel-production-test")
                .arg("mcp")
                .arg("serve")
                .stderr(Stdio::inherit());
        }),
    )
    .unwrap()
}

#[test]
fn parallel_production_package_keeps_stock_generated_and_voice_lanes_truthful() {
    let contract = contract();
    assert_eq!(contract["schema"], "omnicreator.agent-parallel-production");
    assert_eq!(contract["version"], 1);
    assert_eq!(
        contract["concurrency"]["persisted_as_project_truth"],
        false
    );
    assert_eq!(
        contract["concurrency"]["canonical_commit"],
        "serialize through the coordinator and Data Root writer lease"
    );

    let stock = &contract["visual"]["stock"];
    assert_eq!(stock["example_provider"], "pexels");
    assert_eq!(stock["search_operation"], "visual.resolve");
    assert_eq!(stock["fetch_operation"], "visual.fetch_selected");
    assert_eq!(stock["search_policy"], "metadata_and_preview_first");
    assert_eq!(stock["full_media_policy"], "download_only_after_selection");

    let capabilities = stock["required_capabilities"]
        .as_array()
        .expect("stock required_capabilities must be an array")
        .iter()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    for expected in [
        "stock_video",
        "stock_image",
        "preview_first_search",
        "selected_asset_download",
    ] {
        assert!(capabilities.contains(expected), "missing stock capability {expected}");
    }

    let pexels_manifest = read_repo("plugins/pexels/plugin.yaml");
    for capability in &capabilities {
        assert!(
            pexels_manifest.contains(capability),
            "P4 contract references Pexels capability absent from plugin manifest: {capability}"
        );
    }

    let generated = &contract["visual"]["generated"];
    assert_eq!(generated["operation"], "visual.generate");
    assert_eq!(generated["prepare"], "agent_work_prepare");
    assert_eq!(generated["commit"], "agent_work_commit");
    assert_eq!(
        generated["accepted_media"],
        serde_json::json!(["image/png", "image/jpeg", "image/webp"])
    );

    let voice = &contract["voice"]["omnivoice_studio"];
    assert_eq!(voice["operation"], "tts.generate");
    assert_eq!(voice["prepare"], "agent_work_prepare");
    assert_eq!(voice["commit"], "agent_work_commit");
    assert_eq!(voice["source_label"], "omnivoice-studio");
    assert_eq!(voice["timing_required"], true);
    assert_eq!(
        voice["accepted_audio"],
        serde_json::json!(["audio/wav", "audio/mpeg"])
    );

    let agents_skill = read_repo(".agents/skills/omnicreator/SKILL.md");
    let claude_skill = read_repo(".claude/skills/omnicreator/SKILL.md");
    assert_eq!(agents_skill, claude_skill, "skill copies must not drift");
    for required in [
        "Pexels",
        "preview-first",
        "visual.fetch_selected",
        "OmniVoiceStudio",
        "omnivoice-studio",
        "Parallel execution, serialized canonical commits",
    ] {
        assert!(
            agents_skill.contains(required),
            "skill is missing P4 production guidance: {required}"
        );
    }

    for path in [
        "agent-harness/parallel-production/README.md",
        "agent-harness/parallel-production/README.vi.md",
        "agent-harness/parallel-production/contract.json",
    ] {
        let raw = read_repo(path);
        for forbidden in ["sk-or-v1-", "sk-proj-", "BEGIN PRIVATE KEY", "Bearer test-"] {
            assert!(
                !raw.contains(forbidden),
                "parallel production package contains a secret-like marker in {path}: {forbidden}"
            );
        }
    }
}

#[tokio::test]
async fn parallel_production_recipe_references_tools_exposed_by_real_mcp_server() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data-root");
    Workspace::create(&root).unwrap();

    let client = ().serve(mcp_transport(&root)).await.unwrap();
    let discovered = client
        .list_tools(Default::default())
        .await
        .unwrap()
        .tools
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect::<BTreeSet<_>>();

    for tool in [
        "agent_work_graph",
        "agent_work_prepare",
        "agent_work_commit",
        "visual_control",
        "voice_control",
    ] {
        assert!(
            discovered.contains(tool),
            "parallel production recipe references MCP tool not exposed by real binary: {tool}"
        );
    }

    client.cancel().await.unwrap();
}
