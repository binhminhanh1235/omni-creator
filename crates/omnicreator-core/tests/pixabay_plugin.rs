use std::{fs, path::PathBuf};

use omnicreator_core::{
    deterministic_input_hash, load_plugin_settings_ui, scan_plugin_roots, ArtifactStore,
    LogicalUri, PluginJobWorkspace, PluginSettingVisibility, SelectedVisualOutput, StateStore,
    StepStatus, Workspace,
};

#[test]
fn repository_pixabay_plugin_is_discoverable_and_settings_are_valid() {
    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");

    let report = scan_plugin_roots(&[plugin_root]);
    assert!(
        report.diagnostics.is_empty(),
        "unexpected plugin diagnostics: {:?}",
        report.diagnostics
    );

    let plugin = report
        .registry
        .get("pixabay")
        .expect("repository Pixabay plugin must be discoverable");

    assert_eq!(plugin.manifest.api_version, 1);
    assert_eq!(plugin.manifest.types, vec!["visual"]);
    assert!(plugin
        .manifest
        .capabilities
        .contains(&"stock_image".to_owned()));
    assert!(plugin
        .manifest
        .capabilities
        .contains(&"stock_video".to_owned()));
    assert!(plugin
        .manifest
        .capabilities
        .contains(&"preview_first_search".to_owned()));
    assert!(plugin
        .manifest
        .permissions
        .filesystem
        .contains(&"job-workspace".to_owned()));
    assert!(plugin
        .manifest
        .permissions
        .filesystem
        .contains(&"provider-cache".to_owned()));
    assert_eq!(
        plugin.manifest.permissions.network,
        vec!["pixabay.com".to_owned(), "cdn.pixabay.com".to_owned()]
    );

    let settings = load_plugin_settings_ui(plugin);
    assert!(
        settings.diagnostics.is_empty(),
        "unexpected settings diagnostics: {:?}",
        settings.diagnostics
    );

    let ui = settings.ui.expect("Pixabay settings UI must be generated");
    assert_eq!(ui.plugin_id, "pixabay");
    assert_eq!(ui.schema_ref, "settings.schema.json");

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

    let safe_search = ui
        .fields
        .iter()
        .find(|field| field.key == "safe_search")
        .expect("safe_search field");
    assert_eq!(safe_search.visibility, PluginSettingVisibility::Advanced);
}

#[test]
fn selected_pixabay_output_is_verified_and_promoted_by_core() {
    let temp = tempfile::tempdir().unwrap();
    let data_root = temp.path().join("data");
    let workspace = Workspace::create(&data_root).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Pixabay Promotion").unwrap();
    let input_hash = deterministic_input_hash(&[b"pixabay", b"SC17", b"1253"]);
    let job = state
        .create_job(&project.id, "visual", "SC17", &input_hash)
        .unwrap();

    let plugin_workspace =
        PluginJobWorkspace::create(temp.path().join("plugin-runtime"), &job.job_id).unwrap();
    let relative_output = "selected/pixabay-video-1253.mp4";
    let output = plugin_workspace.resolve_output(relative_output).unwrap();
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(&output, b"selected pixabay video fixture").unwrap();

    let selected: SelectedVisualOutput = serde_json::from_value(serde_json::json!({
        "source_provider": "pixabay",
        "source_asset_id": "1253",
        "selection_ref": "pixabay:video:1253",
        "media_type": "video",
        "relative_output": relative_output,
        "width": 1920,
        "height": 1080,
        "duration": 12.0,
        "provenance": {
            "provider": "pixabay",
            "provider_asset_id": "1253",
            "source_page_url": "https://pixabay.com/videos/forest-mist-trees-1253/",
            "creator_name": "VideoCreator",
            "creator_url": "https://pixabay.com/users/VideoCreator-321/",
            "attribution": "Video by VideoCreator on Pixabay",
            "license": "Pixabay Content License"
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
    assert_eq!(artifact.metadata["source_provider"], "pixabay");
    assert_eq!(
        artifact.metadata["provenance"]["creator_name"],
        "VideoCreator"
    );
    assert_eq!(
        artifact.metadata["provenance"]["license"],
        "Pixabay Content License"
    );
}
