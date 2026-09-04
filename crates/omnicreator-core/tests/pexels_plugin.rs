use std::path::PathBuf;

use omnicreator_core::{
    load_plugin_settings_ui, scan_plugin_roots, PluginSettingVisibility,
};

#[test]
fn repository_pexels_plugin_is_discoverable_and_settings_are_valid() {
    let plugin_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");

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
        vec!["api.pexels.com".to_owned()]
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
