use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};

use omnicreator_core::{LlmProviderConfigV1, Workspace};
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

fn json_server(path: &str) -> Value {
    let value: Value = serde_json::from_str(&read_repo(path)).unwrap();
    value["mcpServers"]["omnicreator"].clone()
}

fn assert_stdio_server(server: &Value) {
    assert_eq!(server["command"], "omnicreator");
    let args = server["args"].as_array().expect("mcp args");
    let args = args
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        args,
        vec![
            "--data-root",
            "/ABSOLUTE/PATH/TO/OMNICREATOR_DATA_ROOT",
            "mcp",
            "serve"
        ]
    );
}

fn fixture() -> Value {
    serde_json::from_str(&read_repo(
        "agent-harness/fixtures/blocked-workflow-recovery.json",
    ))
    .unwrap()
}

fn mcp_transport(root: &Path) -> TokioChildProcess {
    TokioChildProcess::new(
        Command::new(env!("CARGO_BIN_EXE_omnicreator")).configure(|command| {
            command
                .arg("--data-root")
                .arg(root)
                .arg("--device-id")
                .arg("phase18-p4-agent-harness-test")
                .arg("mcp")
                .arg("serve")
                .stderr(Stdio::inherit());
        }),
    )
    .unwrap()
}

#[test]
fn checked_in_harness_packages_are_safe_and_in_sync() {
    let agents_skill = read_repo(".agents/skills/omnicreator/SKILL.md");
    let claude_skill = read_repo(".claude/skills/omnicreator/SKILL.md");
    assert_eq!(agents_skill, claude_skill, "skill copies must not drift");

    for required in [
        "Inspect before mutating",
        "Never invent canonical IDs",
        "re-read the relevant status",
        "read_only",
        "writer_conflict",
        "provider_unavailable",
        "Automatic execution being OFF does not mean SKIPPED or SUCCEEDED",
        "Never edit OmniCreator SQLite",
        "Never claim provider, plugin, TTS, GPU, or ComputeProvider success",
        "Review Center/recovery",
        "creator_start_or_resume",
        "production_control",
    ] {
        assert!(
            agents_skill.contains(required),
            "skill is missing required safety guidance: {required}"
        );
    }

    let claude = json_server("agent-harness/claude/.mcp.json.example");
    let antigravity = json_server("agent-harness/antigravity/mcp_config.json.example");
    assert_stdio_server(&claude);
    assert_stdio_server(&antigravity);
    assert_eq!(
        claude, antigravity,
        "JSON stdio examples must stay equivalent"
    );

    let codex = read_repo("agent-harness/codex/config.toml.example");
    assert!(codex.contains("[mcp_servers.omnicreator]"));
    assert!(codex.contains("command = \"omnicreator\""));
    assert!(codex.contains(
        "args = [\"--data-root\", \"/ABSOLUTE/PATH/TO/OMNICREATOR_DATA_ROOT\", \"mcp\", \"serve\"]"
    ));

    for provider_path in [
        "agent-harness/providers/openrouter.json.example",
        "agent-harness/providers/llmgateway.json.example",
    ] {
        let raw = read_repo(provider_path);
        let config: LlmProviderConfigV1 = serde_json::from_str(&raw).unwrap();
        config.validate_v1().unwrap();
        assert!(!raw.contains("\"api_key\":"));
        assert!(!raw.contains("sk-or-"));
        assert!(!raw.contains("Bearer "));
    }

    let flow = fixture();
    assert_eq!(flow["schema"], "omnicreator.agent-harness-fixture");
    let steps = flow["steps"].as_array().expect("fixture steps");
    for (index, step) in steps.iter().enumerate() {
        assert_eq!(step["sequence"].as_u64(), Some((index + 1) as u64));
        if step["kind"] == "mutate" {
            let next = steps
                .get(index + 1)
                .expect("every fixture mutation must be followed by inspection");
            assert_eq!(
                next["kind"],
                "inspect",
                "fixture mutation at sequence {} must be followed by inspection",
                index + 1
            );
        }
    }
    assert_eq!(
        steps[4]["expected"], "provider_unavailable",
        "blocked workflow must preserve truthful provider failure"
    );
    assert_eq!(steps[6]["enabled"], false);
    assert!(steps[9]["expected"]
        .as_str()
        .unwrap()
        .contains("automatic_execution_enabled=false"));

    let all_package_text = [
        agents_skill,
        read_repo("agent-harness/README.md"),
        codex,
        read_repo("agent-harness/claude/.mcp.json.example"),
        read_repo("agent-harness/antigravity/mcp_config.json.example"),
        read_repo("agent-harness/providers/openrouter.json.example"),
        read_repo("agent-harness/providers/llmgateway.json.example"),
        read_repo("agent-harness/fixtures/blocked-workflow-recovery.json"),
    ]
    .join("\n");
    for forbidden in ["sk-or-v1-", "sk-proj-", "BEGIN PRIVATE KEY", "Bearer test-"] {
        assert!(
            !all_package_text.contains(forbidden),
            "agent package contains a secret-like marker: {forbidden}"
        );
    }
}

#[tokio::test]
async fn blocked_workflow_fixture_only_references_tools_on_real_mcp_server() {
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

    let flow = fixture();
    for step in flow["steps"].as_array().unwrap() {
        let tool = step["tool"].as_str().unwrap();
        assert!(
            discovered.contains(tool),
            "fixture references MCP tool not exposed by the real binary: {tool}"
        );
    }

    for required in [
        "workspace_status",
        "creator_start_or_resume",
        "workflow_set_step_auto",
        "content_control",
        "scene_plan_control",
        "visual_control",
        "voice_control",
        "review_list",
        "production_control",
    ] {
        assert!(discovered.contains(required));
    }

    client.cancel().await.unwrap();
}
