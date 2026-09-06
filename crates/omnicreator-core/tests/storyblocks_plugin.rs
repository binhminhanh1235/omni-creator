use std::{fs, path::PathBuf};

use omnicreator_core::{
    deterministic_input_hash, load_plugin_settings_ui, scan_plugin_roots, ArtifactStore,
    LogicalUri, PluginJobWorkspace, PluginSettingVisibility, SelectedVisualOutput, StateStore,
    StepStatus, Workspace,
};

#[test]
fn repository_storyblocks_plugin_is_discoverable_and_license_gated() {
    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");

    let report = scan_plugin_roots(&[plugin_root]);
    assert!(
        report.diagnostics.is_empty(),
        "unexpected plugin diagnostics: {:?}",
        report.diagnostics
    );

    let plugin = report
        .registry
        .get("storyblocks")
        .expect("repository Storyblocks plugin must be discoverable");

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
        .capabilities
        .contains(&"selected_asset_download".to_owned()));
    assert!(plugin
        .manifest
        .capabilities
        .contains(&"license_gated_download".to_owned()));
    assert_eq!(
        plugin.manifest.permissions.filesystem,
        vec!["job-workspace".to_owned()]
    );
    assert_eq!(
        plugin.manifest.permissions.network,
        vec![
            "api.storyblocks.com".to_owned(),
            "d2v9y0dukr6mq2.cloudfront.net".to_owned(),
            "d1yn1kh78jj1rr.cloudfront.net".to_owned(),
        ]
    );

    let settings = load_plugin_settings_ui(plugin);
    assert!(
        settings.diagnostics.is_empty(),
        "unexpected settings diagnostics: {:?}",
        settings.diagnostics
    );
    let ui = settings
        .ui
        .expect("Storyblocks settings UI must be generated");
    assert_eq!(ui.plugin_id, "storyblocks");
    assert_eq!(ui.schema_ref, "settings.schema.json");

    let media_type = ui
        .fields
        .iter()
        .find(|field| field.key == "media_type")
        .expect("media_type field");
    assert_eq!(media_type.visibility, PluginSettingVisibility::Basic);

    for key in [
        "public_key_env",
        "private_key_env",
        "user_id_env",
        "api_mode_env",
    ] {
        let field = ui
            .fields
            .iter()
            .find(|field| field.key == key)
            .unwrap_or_else(|| panic!("missing Storyblocks settings field {key}"));
        assert_eq!(field.visibility, PluginSettingVisibility::Advanced);
    }
}

#[test]
fn selected_storyblocks_output_is_verified_and_promoted_by_core() {
    let temp = tempfile::tempdir().unwrap();
    let data_root = temp.path().join("data");
    let workspace = Workspace::create(&data_root).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Storyblocks Promotion").unwrap();
    let input_hash = deterministic_input_hash(&[b"storyblocks", b"SC17", b"11851"]);
    let job = state
        .create_job(&project.id, "visual", "SC17", &input_hash)
        .unwrap();

    let plugin_workspace =
        PluginJobWorkspace::create(temp.path().join("plugin-runtime"), &job.job_id).unwrap();
    let relative_output = "selected/storyblocks-video-11851.mp4";
    let output = plugin_workspace.resolve_output(relative_output).unwrap();
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(&output, b"selected storyblocks video fixture").unwrap();

    let selected: SelectedVisualOutput = serde_json::from_value(serde_json::json!({
        "source_provider": "storyblocks",
        "source_asset_id": "11851",
        "selection_ref": "storyblocks:video:11851",
        "media_type": "video",
        "relative_output": relative_output,
        "width": 1920,
        "height": 1080,
        "duration": 12.0,
        "provenance": {
            "provider": "storyblocks",
            "provider_asset_id": "11851",
            "provider_asset_code": "SBV-71090305",
            "title": "Quiet Forest Dawn",
            "creator_name": "Storyblocks Video",
            "license": "Storyblocks production API license",
            "license_mode": "production_api",
            "licensed_project_id": project.id,
            "selected_download_recorded": true,
            "download_format": "MP4",
            "provider_rendition": "_1080p"
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
    assert_eq!(artifact.metadata["source_provider"], "storyblocks");
    assert_eq!(
        artifact.metadata["provenance"]["license_mode"],
        "production_api"
    );
    assert_eq!(
        artifact.metadata["provenance"]["licensed_project_id"],
        project.id
    );
    assert_eq!(
        artifact.metadata["provenance"]["selected_download_recorded"],
        true
    );
}
