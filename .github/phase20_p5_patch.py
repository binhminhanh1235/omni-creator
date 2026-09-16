from pathlib import Path


def replace_once(path: str, old: str, new: str, label: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one match, found {count}")
    file.write_text(text.replace(old, new, 1))


# Keep the persisted credential implementation in the established application module.
main_path = "apps/desktop/src-tauri/src/main.rs"
replace_once(
    main_path,
    '    include!("application.rs");\n    include!("phase17.rs");',
    '    include!("application.rs");\n    include!("plugin_credentials.rs");\n    include!("phase17.rs");',
    "main include",
)

credential_impl = r'''#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedPluginCredentialsV1 {
    #[serde(default = "plugin_credentials_version_v1")]
    version: u32,
    #[serde(default)]
    values: BTreeMap<String, String>,
}

fn plugin_credentials_version_v1() -> u32 {
    1
}

fn plugin_credentials_path_v1(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("Cannot resolve app config directory: {error}"))?;
    fs::create_dir_all(&config_dir)
        .map_err(|error| format!("Cannot create app config directory: {error}"))?;
    Ok(config_dir.join("plugin-credentials.json"))
}

fn load_plugin_credentials_file_v1(path: &Path) -> Result<BTreeMap<String, String>, String> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let bytes = fs::read(path)
        .map_err(|error| format!("Cannot read machine-local plugin credentials: {error}"))?;
    let stored: PersistedPluginCredentialsV1 = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid machine-local plugin credential store: {error}"))?;
    if stored.version != plugin_credentials_version_v1() {
        return Err(format!(
            "Unsupported plugin credential store version {}.",
            stored.version
        ));
    }
    Ok(stored.values)
}

fn save_plugin_credentials_file_v1(
    path: &Path,
    values: &BTreeMap<String, String>,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Plugin credential store path has no parent.".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Cannot create plugin credential directory: {error}"))?;
    let stored = PersistedPluginCredentialsV1 {
        version: plugin_credentials_version_v1(),
        values: values.clone(),
    };
    let bytes = serde_json::to_vec(&stored)
        .map_err(|error| format!("Cannot encode machine-local plugin credentials: {error}"))?;
    fs::write(path, bytes)
        .map_err(|error| format!("Cannot save machine-local plugin credentials: {error}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
            format!("Cannot restrict plugin credential store permissions: {error}")
        })?;
    }
    Ok(())
}

fn load_persisted_plugin_credentials_v1(
    app: &AppHandle,
) -> Result<BTreeMap<String, String>, String> {
    load_plugin_credentials_file_v1(&plugin_credentials_path_v1(app)?)
}

fn save_persisted_plugin_credentials_v1(
    app: &AppHandle,
    values: &BTreeMap<String, String>,
) -> Result<(), String> {
    save_plugin_credentials_file_v1(&plugin_credentials_path_v1(app)?, values)
}

fn hydrate_persisted_plugin_credentials_v1(
    app: &AppHandle,
    state: &State<'_, DesktopState>,
) -> Result<(), String> {
    let saved = load_persisted_plugin_credentials_v1(app)?;
    let mut guard = state.runtime_plugin_credentials.lock().map_err(lock_error)?;
    *guard = saved;
    Ok(())
}

fn set_persisted_plugin_credential_v1(
    app: &AppHandle,
    state: &State<'_, DesktopState>,
    env_name: &str,
    value: String,
) -> Result<(), String> {
    let mut saved = load_persisted_plugin_credentials_v1(app)?;
    saved.insert(env_name.to_owned(), value);
    save_persisted_plugin_credentials_v1(app, &saved)?;
    let mut guard = state.runtime_plugin_credentials.lock().map_err(lock_error)?;
    *guard = saved;
    Ok(())
}

fn clear_persisted_plugin_credential_v1(
    app: &AppHandle,
    state: &State<'_, DesktopState>,
    env_name: &str,
) -> Result<(), String> {
    let mut saved = load_persisted_plugin_credentials_v1(app)?;
    saved.remove(env_name);
    save_persisted_plugin_credentials_v1(app, &saved)?;
    let mut guard = state.runtime_plugin_credentials.lock().map_err(lock_error)?;
    *guard = saved;
    Ok(())
}

#[cfg(test)]
mod plugin_credential_store_tests {
    use super::*;

    fn temp_store_path_v1(label: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "omnicreator-plugin-credentials-{label}-{}.json",
            Uuid::new_v4().simple()
        ))
    }

    #[test]
    fn persisted_plugin_credentials_survive_reload_and_replace() {
        let path = temp_store_path_v1("replace");
        let mut values = BTreeMap::new();
        values.insert("PEXELS_API_KEY".to_owned(), "first-secret".to_owned());
        save_plugin_credentials_file_v1(&path, &values).unwrap();
        assert_eq!(
            load_plugin_credentials_file_v1(&path)
                .unwrap()
                .get("PEXELS_API_KEY")
                .map(String::as_str),
            Some("first-secret")
        );

        values.insert("PEXELS_API_KEY".to_owned(), "replacement-secret".to_owned());
        save_plugin_credentials_file_v1(&path, &values).unwrap();
        assert_eq!(
            load_plugin_credentials_file_v1(&path)
                .unwrap()
                .get("PEXELS_API_KEY")
                .map(String::as_str),
            Some("replacement-secret")
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn clearing_saved_key_removes_it_from_reload() {
        let path = temp_store_path_v1("clear");
        let mut values = BTreeMap::new();
        values.insert("PEXELS_API_KEY".to_owned(), "secret".to_owned());
        save_plugin_credentials_file_v1(&path, &values).unwrap();
        values.remove("PEXELS_API_KEY");
        save_plugin_credentials_file_v1(&path, &values).unwrap();
        assert!(load_plugin_credentials_file_v1(&path).unwrap().is_empty());
        fs::remove_file(path).unwrap();
    }
}
'''
Path("apps/desktop/src-tauri/src/plugin_credentials.rs").write_text(credential_impl)

app_path = "apps/desktop/src-tauri/src/application.rs"
replace_once(
    app_path,
    '''    if runtime_credentials\n        .get(env_name)\n        .is_some_and(|value| !value.trim().is_empty())\n    {\n        "runtime"\n''',
    '''    if runtime_credentials\n        .get(env_name)\n        .is_some_and(|value| !value.trim().is_empty())\n    {\n        "saved"\n''',
    "saved source label",
)
replace_once(
    app_path,
    '''    state\n        .runtime_plugin_credentials\n        .lock()\n        .map_err(lock_error)?\n        .insert(credential_env.to_owned(), value);\n    plugin_inventory_view_v1(&app, &state)\n''',
    '''    set_persisted_plugin_credential_v1(&app, &state, credential_env, value)?;\n    plugin_inventory_view_v1(&app, &state)\n''',
    "persist set command",
)
replace_once(
    app_path,
    '''    state\n        .runtime_plugin_credentials\n        .lock()\n        .map_err(lock_error)?\n        .remove(credential_env);\n    plugin_inventory_view_v1(&app, &state)\n''',
    '''    clear_persisted_plugin_credential_v1(&app, &state, credential_env)?;\n    plugin_inventory_view_v1(&app, &state)\n''',
    "persist clear command",
)
replace_once(
    app_path,
    '''fn studio_pack_runtime_snapshot_v1(\n    app: &AppHandle,\n    state: &State<'_, DesktopState>,\n    registry: &PluginRegistry,\n) -> Result<StudioPackRuntimeSnapshotV1, String> {\n    let lifecycle = load_plugin_lifecycle_v1(app)?;\n    let runtime_credentials = runtime_plugin_credentials_snapshot_v1(state)?;\n''',
    '''fn studio_pack_runtime_snapshot_v1(\n    app: &AppHandle,\n    state: &State<'_, DesktopState>,\n    registry: &PluginRegistry,\n) -> Result<StudioPackRuntimeSnapshotV1, String> {\n    hydrate_persisted_plugin_credentials_v1(app, state)?;\n    let lifecycle = load_plugin_lifecycle_v1(app)?;\n    let runtime_credentials = runtime_plugin_credentials_snapshot_v1(state)?;\n''',
    "hydrate readiness snapshot",
)

js_path = "apps/desktop/dist/app.js"
replace_once(
    js_path,
    '''        const sourceLabel =\n          source === "runtime"\n            ? "Runtime key active"\n            : source === "environment"\n              ? "OS environment active"\n              : "Key required";\n        const placeholder =\n          source === "runtime"\n            ? "Paste a replacement key for this session"\n            : source === "environment"\n              ? "Paste a temporary override for this session"\n              : "Paste API key for this OmniCreator session";\n''',
    '''        const sourceLabel =\n          source === "saved"\n            ? "Saved key active"\n            : source === "environment"\n              ? "OS environment active"\n              : "Key required";\n        const placeholder =\n          source === "saved"\n            ? "Paste a replacement API key"\n            : source === "environment"\n              ? "Paste a saved override for this device"\n              : "Paste API key to save on this device";\n''',
    "credential source UI",
)
replace_once(
    js_path,
    '''          '" aria-label="Runtime API key for ' +\n          escapeHtml(credential.env_name) +\n          '" /><button class="btn primary plugin-runtime-key-set" type="button">Set runtime key</button>' +\n          (source === "runtime"\n            ? '<button class="btn plugin-runtime-key-clear" type="button">Clear runtime key</button>'\n            : "") +\n          '</div><small>Memory only. Cleared when OmniCreator exits. The key is never written to the Data Root.</small></div>'\n''',
    '''          '" aria-label="API key for ' +\n          escapeHtml(credential.env_name) +\n          '" /><button class="btn primary plugin-runtime-key-set" type="button">' +\n          (source === "saved" ? "Replace saved key" : "Save API key") +\n          '</button>' +\n          (source === "saved"\n            ? '<button class="btn plugin-runtime-key-clear" type="button">Clear saved key</button>'\n            : "") +\n          '</div><small>Saved on this device outside the Data Root. The key is never written to project data or shown again.</small></div>'\n''',
    "credential controls UI",
)
replace_once(
    js_path,
    '''        const next = await call("set_plugin_runtime_credential", {\n          pluginId: row.dataset.pluginId,\n          credentialEnv: row.dataset.credentialEnv,\n          value: value,\n        });\n        const readiness = pluginReadiness(next, row.dataset.pluginId);\n''',
    '''        await call("set_plugin_runtime_credential", {\n          pluginId: row.dataset.pluginId,\n          credentialEnv: row.dataset.credentialEnv,\n          value: value,\n        });\n        const next = await call("plugin_inventory");\n        const readiness = pluginReadiness(next, row.dataset.pluginId);\n''',
    "fresh set inventory",
)
replace_once(
    js_path,
    'showToast("Runtime API key applied for this OmniCreator session.");',
    'showToast("API key saved on this device and readiness refreshed.");',
    "set toast",
)
replace_once(
    js_path,
    '''      const next = await call("clear_plugin_runtime_credential", {\n        pluginId: row.dataset.pluginId,\n        credentialEnv: row.dataset.credentialEnv,\n      });\n      const plugin = (next.plugins || []).find(function (item) {\n''',
    '''      await call("clear_plugin_runtime_credential", {\n        pluginId: row.dataset.pluginId,\n        credentialEnv: row.dataset.credentialEnv,\n      });\n      const next = await call("plugin_inventory");\n      const plugin = (next.plugins || []).find(function (item) {\n''',
    "fresh clear inventory",
)
replace_once(
    js_path,
    'showToast("Runtime API key cleared. Environment fallback was re-evaluated.");',
    'showToast("Saved API key cleared. Environment fallback was re-evaluated.");',
    "clear toast",
)

# Strengthen the regression contract: persisted local store + canonical refresh + no secret projection.
test_path = Path("apps/desktop/tests/plugin-runtime-credentials-smoke.mjs")
test = test_path.read_text()
test = test.replace(
    'assert.match(app, /Memory only\\. Cleared when OmniCreator exits/);',
    'assert.match(app, /Saved on this device outside the Data Root/);',
)
test = test.replace(
    'assert.match(rust, /runtime_plugin_credentials: Mutex<BTreeMap<String, String>>/);',
    'assert.match(rust, /runtime_plugin_credentials: Mutex<BTreeMap<String, String>>/);\nassert.match(rust, /hydrate_persisted_plugin_credentials_v1\\(app, state\\)/);',
)
test = test.replace(
    'assert.match(rust, /runtime_credential_source_v1/);',
    'assert.match(rust, /runtime_credential_source_v1/);\nassert.match(app, /const next = await call\\("plugin_inventory"\\)/);\nassert.match(app, /Saved key active/);\nassert.match(app, /Replace saved key/);',
)
test = test.replace(
    'const core = fs.readFileSync(new URL("../../../crates/omnicreator-core/src/plugin_process.rs", import.meta.url), "utf8");',
    'const core = fs.readFileSync(new URL("../../../crates/omnicreator-core/src/plugin_process.rs", import.meta.url), "utf8");\nconst credentials = fs.readFileSync(new URL("../src-tauri/src/plugin_credentials.rs", import.meta.url), "utf8");',
)
test = test.replace(
    'assert.match(core, /\\.envs\\(environment\\)/);',
    'assert.match(core, /\\.envs\\(environment\\)/);\nassert.match(credentials, /plugin-credentials\\.json/);\nassert.match(credentials, /save_persisted_plugin_credentials_v1/);\nassert.match(credentials, /set_persisted_plugin_credential_v1/);\nassert.match(credentials, /clear_persisted_plugin_credential_v1/);\nassert.doesNotMatch(app, /first-secret|replacement-secret/);',
)
test_path.write_text(test)

# Update docs to reflect the user-selected persisted machine-local credential behavior.
for path, old, new in [
    (
        "docs/25-unified-settings.md",
        "Runtime API keys entered on plugin cards are memory-only for the current Desktop process. They are cleared when OmniCreator exits and are never written to the Data Root.",
        "Plugin API keys entered on plugin cards are saved in a machine-local Desktop credential file outside the Data Root, reloaded after restart, and never projected back to the UI. Setting a key again replaces the saved value; clearing it removes the saved value and re-evaluates the OS environment fallback.",
    ),
    (
        "docs/25-unified-settings.vi.md",
        "Giá trị credential vẫn chỉ nằm trên máy. OmniCreator chỉ lưu tên environment variable và runtime setting machine-local; secret value không được ghi vào Data Root hoặc canonical project artifact.",
        "Giá trị credential vẫn chỉ nằm trên máy và không đi vào Data Root/canonical project artifact. API key nhập trực tiếp ở card plugin được lưu trong credential file machine-local của Desktop để dùng lại sau khi restart; UI không đọc ngược secret ra input. Nhập key mới sẽ thay key đã lưu; Clear xóa key đã lưu và quay về environment variable nếu có.",
    ),
]:
    file = Path(path)
    text = file.read_text()
    if old not in text:
        if path.endswith(".md") and not path.endswith(".vi.md"):
            # Older EN doc did not yet include P4 follow-up wording; append a focused section.
            text += "\n\n## Persisted plugin API keys\n\n" + new + "\n"
        else:
            raise SystemExit(f"docs replacement missing: {path}")
    else:
        text = text.replace(old, new, 1)
    file.write_text(text)
