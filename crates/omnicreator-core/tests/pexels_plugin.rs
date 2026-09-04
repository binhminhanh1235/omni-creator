use std::{fs, path::PathBuf};

use omnicreator_core::{
    deterministic_input_hash, load_plugin_settings_ui, scan_plugin_roots, ArtifactStore,
    LogicalUri, PluginJobWorkspace, PluginSettingVisibility, SelectedVisualOutput, StateStore,
    StepStatus, Workspace,
};

#[test]
fn repository_pexels_plugin_is_discoverable_and_settings_are_valid() {
    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");

    let report = scan_plugin_roots(&[plugin_root]);
    assert!(
        report.diagnostics.is_empty(),
        "unexpected plugin diagnostics: {:?}",
        report.diagnostics
    );

    let plugin = report
        .registry
        .get("pexels")
        .expect("repository Pexels plugin must be discoverable");

    assert_eq!(plugin.manifest.api_version, 1);
    assert_eq!(plugin.manifest.types, vec!["visual"]);
    assert!(plugin
        .manifest
        .capabilities
        .contains(&"preview_first_search".to_owned()));
    assert_eq!(
        plugin.manifest.permissions.network,
        vec![
            "api.pexels.com".to_owned(),
            "images.pexels.com".to_owned(),
            "player.vimeo.com".to_owned(),
        ]
    );
    assert_eq!(
        plugin.manifest.permissions.filesystem,
        vec!["job-workspace".to_owned()]
    );

    let settings = load_plugin_settings_ui(plugin);
    assert!(
        settings.diagnostics.is_empty(),
        "unexpected settings diagnostics: {:?}",
        settings.diagnostics
    );

    let ui = settings.ui.expect("Pexels settings UI must be generated");
    assert_eq!(ui.plugin_id, "pexels");
    assert_eq!(ui.schema_ref, "settings.schema.json");

    let keys = ui
        .fields
        .iter()
        .map(|field| field.key.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        vec![
            "api_key_env",
            "locale",
            "media_type",
            "minimum_size",
            "orientation",
            "per_query"
        ]
    );

    let api_key_env = ui
        .fields
        .iter()
        .find(|field| field.key == "api_key_env")
        .expect("api_key_env field");
    assert_eq!(api_key_env.visibility, PluginSettingVisibility::Advanced);

    let media_type = ui
        .fields
        .iter()
        .find(|field| field.key == "media_type")
        .expect("media_type field");
    assert_eq!(media_type.visibility, PluginSettingVisibility::Basic);
}

#[test]
fn selected_pexels_output_is_verified_and_promoted_by_core() {
    let temp = tempfile::tempdir().unwrap();
    let data_root = temp.path().join("data");
    let workspace = Workspace::create(&data_root).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Pexels Promotion").unwrap();
    let input_hash = deterministic_input_hash(&[b"pexels", b"SC17", b"2499611"]);
    let job = state
        .create_job(&project.id, "visual", "SC17", &input_hash)
        .unwrap();

    let plugin_workspace =
        PluginJobWorkspace::create(temp.path().join("plugin-runtime"), &job.job_id).unwrap();
    let relative_output = "selected/pexels-video-2499611.mp4";
    let output = plugin_workspace.resolve_output(relative_output).unwrap();
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(&output, b"selected pexels video fixture").unwrap();

    let selected: SelectedVisualOutput = serde_json::from_value(serde_json::json!({
        "source_provider": "pexels",
        "source_asset_id": "2499611",
        "selection_ref": "pexels:video:2499611",
        "media_type": "video",
        "relative_output": relative_output,
        "width": 1920,
        "height": 1080,
        "duration": 8.0,
        "provenance": {
            "provider": "pexels",
            "source_page_url": "https://www.pexels.com/video/example-2499611/",
            "creator_name": "Video Creator",
            "creator_url": "https://www.pexels.com/@video-creator",
            "attribution": "Video by Video Creator on Pexels"
        }
    }))
    .unwrap();

    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let artifact = artifacts
        .promote_plugin_output(
            &mut state,
            &job.job_id,
            &plugin_workspace,
            selected
                .promotion(LogicalUri::parse("project://video/SC17.mp4").unwrap())
                .unwrap(),
        )
        .unwrap();

    assert_eq!(
        state.get_job(&job.job_id).unwrap().status,
        StepStatus::Succeeded
    );
    assert!(artifacts.verify_artifact(&artifact).unwrap());
    assert_eq!(artifact.metadata["source_provider"], "pexels");
    assert_eq!(
        artifact.metadata["provenance"]["creator_name"],
        "Video Creator"
    );
    assert_eq!(
        artifact.metadata["provenance"]["attribution"],
        "Video by Video Creator on Pexels"
    );
}
