use std::{fs, path::PathBuf};

use omnicreator_core::{
    deterministic_input_hash, load_plugin_settings_ui, scan_plugin_roots, ArtifactStore,
    LogicalUri, PluginJobWorkspace, PluginSettingVisibility, SelectedVisualOutput, StateStore,
    StepStatus, Workspace,
};

#[test]
fn repository_unsplash_plugin_is_discoverable_and_settings_are_valid() {
    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");

    let report = scan_plugin_roots(&[plugin_root]);
    assert!(
        report.diagnostics.is_empty(),
        "unexpected plugin diagnostics: {:?}",
        report.diagnostics
    );

    let plugin = report
        .registry
        .get("unsplash")
        .expect("repository Unsplash plugin must be discoverable");

    assert_eq!(plugin.manifest.api_version, 1);
    assert_eq!(plugin.manifest.types, vec!["visual"]);
    assert!(plugin
        .manifest
        .capabilities
        .contains(&"stock_image".to_owned()));
    assert!(!plugin
        .manifest
        .capabilities
        .contains(&"stock_video".to_owned()));
    assert!(plugin
        .manifest
        .capabilities
        .contains(&"preview_first_search".to_owned()));
    assert!(plugin
        .manifest
        .capabilities
        .contains(&"selected_asset_download".to_owned()));
    assert_eq!(
        plugin.manifest.permissions.network,
        vec![
            "api.unsplash.com".to_owned(),
            "images.unsplash.com".to_owned(),
            "unsplash.com".to_owned(),
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

    let ui = settings.ui.expect("Unsplash settings UI must be generated");
    assert_eq!(ui.plugin_id, "unsplash");
    assert_eq!(ui.schema_ref, "settings.schema.json");

    let per_query = ui
        .fields
        .iter()
        .find(|field| field.key == "per_query")
        .expect("per_query field");
    assert_eq!(per_query.visibility, PluginSettingVisibility::Basic);

    let api_key_env = ui
        .fields
        .iter()
        .find(|field| field.key == "api_key_env")
        .expect("api_key_env field");
    assert_eq!(api_key_env.visibility, PluginSettingVisibility::Advanced);
}

#[test]
fn selected_unsplash_output_is_verified_and_promoted_by_core() {
    let temp = tempfile::tempdir().unwrap();
    let data_root = temp.path().join("data");
    let workspace = Workspace::create(&data_root).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Unsplash Promotion").unwrap();
    let input_hash = deterministic_input_hash(&[b"unsplash", b"SC17", b"abc_DEF-123"]);
    let job = state
        .create_job(&project.id, "visual", "SC17", &input_hash)
        .unwrap();

    let plugin_workspace =
        PluginJobWorkspace::create(temp.path().join("plugin-runtime"), &job.job_id).unwrap();
    let relative_output = "selected/unsplash-image-abc_DEF-123.jpg";
    let output = plugin_workspace.resolve_output(relative_output).unwrap();
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(&output, b"selected unsplash image fixture").unwrap();

    let selected: SelectedVisualOutput = serde_json::from_value(serde_json::json!({
        "source_provider": "unsplash",
        "source_asset_id": "abc_DEF-123",
        "selection_ref": "unsplash:image:abc_DEF-123",
        "media_type": "image",
        "relative_output": relative_output,
        "width": 4000,
        "height": 2667,
        "duration": null,
        "provenance": {
            "provider": "unsplash",
            "provider_asset_id": "abc_DEF-123",
            "source_page_url": "https://unsplash.com/photos/abc_DEF-123?utm_source=omnicreator&utm_medium=referral",
            "creator_name": "Annie Example",
            "creator_url": "https://unsplash.com/@annie?utm_source=omnicreator&utm_medium=referral",
            "unsplash_url": "https://unsplash.com/?utm_source=omnicreator&utm_medium=referral",
            "attribution": "Photo by Annie Example on Unsplash",
            "license": "Unsplash License",
            "download_tracked": true
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
                .promotion(LogicalUri::parse("project://image/SC17.jpg").unwrap())
                .unwrap(),
        )
        .unwrap();

    assert_eq!(
        state.get_job(&job.job_id).unwrap().status,
        StepStatus::Succeeded
    );
    assert!(artifacts.verify_artifact(&artifact).unwrap());
    assert_eq!(artifact.metadata["source_provider"], "unsplash");
    assert_eq!(
        artifact.metadata["provenance"]["creator_name"],
        "Annie Example"
    );
    assert_eq!(
        artifact.metadata["provenance"]["download_tracked"],
        true
    );
}
