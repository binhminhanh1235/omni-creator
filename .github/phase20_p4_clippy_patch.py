from pathlib import Path

path = Path("apps/desktop/src-tauri/src/application.rs")
text = path.read_text()

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

path.write_text(text)
