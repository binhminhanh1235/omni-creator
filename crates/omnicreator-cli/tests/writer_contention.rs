use std::{io::Cursor, path::Path, process::Stdio};

use omnicreator_cli::run_cli_v1;
use omnicreator_core::{Workspace, WorkspaceSession};
use rmcp::{
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
    ServiceExt,
};
use serde_json::Value;
use tokio::process::Command;

fn cli_args(root: &Path, command: &[&str]) -> Vec<String> {
    let mut args = vec![
        "--data-root".to_owned(),
        root.to_string_lossy().to_string(),
        "--device-id".to_owned(),
        "phase18-p5-cli-contender".to_owned(),
        "--json".to_owned(),
    ];
    args.extend(command.iter().map(|value| (*value).to_owned()));
    args
}

fn mcp_transport(root: &Path, device_id: &str) -> TokioChildProcess {
    TokioChildProcess::new(
        Command::new(env!("CARGO_BIN_EXE_omnicreator")).configure(|command| {
            command
                .arg("--data-root")
                .arg(root)
                .arg("--device-id")
                .arg(device_id)
                .arg("mcp")
                .arg("serve")
                .stderr(Stdio::null());
        }),
    )
    .unwrap()
}

#[tokio::test]
async fn desktop_style_writer_blocks_cli_and_mcp_until_canonical_lease_is_released() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data-root");
    let workspace = Workspace::create(&root).unwrap();
    let desktop_holder = WorkspaceSession::acquire(workspace, "phase18-p5-desktop-holder").unwrap();

    let mut stdin = Cursor::new(Vec::<u8>::new());
    let cli = run_cli_v1(
        cli_args(&root, &["project", "create", "--title", "Must be rejected"]),
        &mut stdin,
    );
    assert_eq!(cli.exit_code, 5, "{}", cli.output);
    let cli_json: Value = serde_json::from_str(&cli.output).unwrap();
    assert_eq!(cli_json["error"]["code"], "writer_conflict");

    let mcp = ().serve(mcp_transport(&root, "phase18-p5-mcp-contender")).await;
    assert!(
        mcp.is_err(),
        "writable MCP startup must fail while another canonical writer lease is active"
    );

    drop(desktop_holder);

    let mcp = ()
        .serve(mcp_transport(&root, "phase18-p5-mcp-after-release"))
        .await
        .expect("MCP must start after the canonical writer lease is released");
    let status = mcp
        .call_tool(CallToolRequestParams::new("workspace_status"))
        .await
        .unwrap();
    assert_eq!(status.is_error, Some(false));
    mcp.cancel().await.unwrap();
}
