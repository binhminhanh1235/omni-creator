from pathlib import Path

app_path = Path("apps/desktop/src-tauri/src/application.rs")
text = app_path.read_text()

old = '''fn runtime_plugin_credentials_snapshot_v1(
    state: &State<'_, DesktopState>,
) -> Result<BTreeMap<String, String>, String> {
    state
        .runtime_plugin_credentials
        .lock()
        .map_err(lock_error)
        .map(|guard| guard.clone())
}
'''
new = '''fn runtime_plugin_credentials_snapshot_v1(
    state: &State<'_, DesktopState>,
) -> Result<BTreeMap<String, String>, String> {
    let guard = state.runtime_plugin_credentials.lock().map_err(lock_error)?;
    Ok(guard.clone())
}
'''
if text.count(old) != 1:
    raise SystemExit(f"runtime credential snapshot match count: {text.count(old)}")
text = text.replace(old, new, 1)

old = '''            .filter(|(name, value)| declared.contains(*name) && !value.trim().is_empty())
'''
new = '''            .filter(|(name, value)| declared.contains(name.as_str()) && !value.trim().is_empty())
'''
if text.count(old) != 1:
    raise SystemExit(f"declared credential filter match count: {text.count(old)}")
text = text.replace(old, new, 1)

app_path.write_text(text)

phase17_path = Path("apps/desktop/src-tauri/src/phase17.rs")
text = phase17_path.read_text()

old = '''        let plugin_runtime = studio_pack_runtime_snapshot_v1(self.app, &inventory.registry)
            .map_err(Self::capability_error_v1)?;
        let runtime_root =
            creator_plugin_runtime_root_v1(self.app).map_err(Self::internal_error_v1)?;
        let visual_runtime = DesktopVisualRuntimeV1 {
            registry: &inventory.registry,
            runtime: &plugin_runtime,
            runtime_root,
        };
'''
new = '''        let plugin_runtime =
            studio_pack_runtime_snapshot_v1(self.app, self.state, &inventory.registry)
                .map_err(Self::capability_error_v1)?;
        let runtime_root =
            creator_plugin_runtime_root_v1(self.app).map_err(Self::internal_error_v1)?;
        let runtime_credentials =
            runtime_plugin_credentials_snapshot_v1(self.state).map_err(Self::internal_error_v1)?;
        let visual_runtime = DesktopVisualRuntimeV1 {
            registry: &inventory.registry,
            runtime: &plugin_runtime,
            runtime_root,
            runtime_credentials,
        };
'''
if text.count(old) != 1:
    raise SystemExit(f"phase17 visual runtime match count: {text.count(old)}")
text = text.replace(old, new, 1)

old = '''            plugin_inventory,
            set_plugin_enabled,
'''
new = '''            plugin_inventory,
            set_plugin_runtime_credential,
            clear_plugin_runtime_credential,
            set_plugin_enabled,
'''
if text.count(old) != 1:
    raise SystemExit(f"phase17 handler match count: {text.count(old)}")
text = text.replace(old, new, 1)

phase17_path.write_text(text)
