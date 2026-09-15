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
    serde_json::from_str(&read_repo("agent-harness/worker-pool/contract.json")).unwrap()
}

fn mcp_transport(root: &Path) -> TokioChildProcess {
    TokioChildProcess::new(
        Command::new(env!("CARGO_BIN_EXE_omnicreator")).configure(|command| {
            command
                .arg("--data-root")
                .arg(root)
                .arg("--device-id")
                .arg("phase19-p3-worker-pool-test")
                .arg("mcp")
                .arg("serve")
                .stderr(Stdio::inherit());
        }),
    )
    .unwrap()
}

#[test]
fn worker_pool_package_preserves_single_canonical_writer_contract() {
    let agents_skill = read_repo(".agents/skills/omnicreator/SKILL.md");
    let claude_skill = read_repo(".claude/skills/omnicreator/SKILL.md");
    assert_eq!(agents_skill, claude_skill, "skill copies must not drift");

    for required in [
        "agent_work_graph",
        "agent_work_prepare",
        "agent_work_commit",
        "Workers never call `agent_work_commit`",
        "Concurrency is harness-local policy, never persisted workflow truth",
        "stale_input",
        "writer_conflict",
    ] {
        assert!(
            agents_skill.contains(required),
            "skill is missing worker-pool guidance: {required}"
        );
    }

    let contract = contract();
    assert_eq!(contract["schema"], "omnicreator.agent-worker-pool");
    assert_eq!(contract["version"], 1);
    assert_eq!(contract["worker"]["canonical_mutation"], false);
    assert_eq!(contract["worker"]["may_edit_data_root"], false);
    assert_eq!(contract["worker"]["may_invent_ids"], false);
    assert_eq!(contract["concurrency"]["default_max_workers"], 4);
    assert_eq!(
        contract["concurrency"]["persisted_as_workflow_truth"],
        false
    );
    assert_eq!(
        contract["coordinator"]["serialize_canonical_commits"],
        true
    );
    assert_eq!(
        contract["coordinator"]["reinspect_after_every_commit"],
        true
    );

    for path in [
        "agent-harness/worker-pool/README.md",
        "agent-harness/codex/WORKER-POOL.md",
        "agent-harness/claude/WORKER-POOL.md",
        "agent-harness/antigravity/WORKER-POOL.md",
    ] {
        let raw = read_repo(path);
        for required in [
            "agent_work_graph",
            "agent_work_prepare",
            "agent_work_commit",
            "coordinator",
            "worker",
        ] {
            assert!(
                raw.contains(required),
                "{path} is missing worker-pool term: {required}"
            );
        }
    }

    let package_text = [
        agents_skill,
        read_repo("agent-harness/worker-pool/README.md"),
        read_repo("agent-harness/codex/WORKER-POOL.md"),
        read_repo("agent-harness/claude/WORKER-POOL.md"),
        read_repo("agent-harness/antigravity/WORKER-POOL.md"),
        read_repo("agent-harness/worker-pool/contract.json"),
    ]
    .join("\n");
    for forbidden in ["sk-or-v1-", "sk-proj-", "BEGIN PRIVATE KEY", "Bearer test-"] {
        assert!(
            !package_text.contains(forbidden),
            "worker-pool package contains a secret-like marker: {forbidden}"
        );
    }
}

#[tokio::test]
async fn worker_pool_coordinator_tools_exist_on_real_mcp_server() {
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

    let contract = contract();
    for field in ["inspect_tool", "prepare_tool", "commit_tool"] {
        let tool = contract["coordinator"][field]
            .as_str()
            .expect("coordinator tool must be a string");
        assert!(
            discovered.contains(tool),
            "worker-pool contract references MCP tool not exposed by real binary: {tool}"
        );
    }

    client.cancel().await.unwrap();
}
