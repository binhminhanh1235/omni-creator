#[derive(Debug, Default, Serialize, Deserialize)]
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
