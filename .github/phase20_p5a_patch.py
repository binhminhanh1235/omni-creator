from pathlib import Path


def replace_once(path: str, old: str, new: str, label: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one match, found {count}")
    file.write_text(text.replace(old, new, 1))


app_path = "apps/desktop/dist/app.js"
old_handlers = r'''  document.querySelectorAll(".plugin-runtime-key-set").forEach(function (button) {
    button.onclick = async function () {
      const row = button.closest(".plugin-runtime-credential");
      const input = row && row.querySelector(".plugin-runtime-key-input");
      const value = input ? input.value : "";
      if (!row || !value.trim()) {
        showToast("Paste an API key before setting the runtime credential.");
        if (input) input.focus();
        return;
      }
      button.disabled = true;
      try {
        await call("set_plugin_runtime_credential", {
          pluginId: row.dataset.pluginId,
          credentialEnv: row.dataset.credentialEnv,
          value: value,
        });
        const next = await call("plugin_inventory");
        const readiness = pluginReadiness(next, row.dataset.pluginId);
        pluginManagerState.tab = readiness.status === "ready" ? "enabled" : "needs_attention";
        await refreshPluginManagerAfterMutation(next);
        showToast("API key saved on this device and readiness refreshed.");
      } finally {
        if (input) input.value = "";
      }
    };
  });

  document.querySelectorAll(".plugin-runtime-key-clear").forEach(function (button) {
    button.onclick = async function () {
      const row = button.closest(".plugin-runtime-credential");
      if (!row) return;
      button.disabled = true;
      await call("clear_plugin_runtime_credential", {
        pluginId: row.dataset.pluginId,
        credentialEnv: row.dataset.credentialEnv,
      });
      const next = await call("plugin_inventory");
      const plugin = (next.plugins || []).find(function (item) {
        return item.id === row.dataset.pluginId;
      });
      pluginManagerState.tab = plugin ? pluginManagerBucket(next, plugin) : "needs_attention";
      await refreshPluginManagerAfterMutation(next);
      showToast("Saved API key cleared. Environment fallback was re-evaluated.");
    };
  });
'''
new_handlers = r'''  document.querySelectorAll(".plugin-runtime-key-set").forEach(function (button) {
    button.onclick = async function () {
      const row = button.closest(".plugin-runtime-credential");
      const input = row && row.querySelector(".plugin-runtime-key-input");
      const value = input ? input.value : "";
      if (!row || !value.trim()) {
        showToast("Paste an API key before saving the credential.");
        if (input) input.focus();
        return;
      }

      const originalLabel = button.textContent;
      let succeeded = false;
      button.disabled = true;
      button.textContent = "Saving…";
      try {
        const next = await call("set_plugin_runtime_credential", {
          pluginId: row.dataset.pluginId,
          credentialEnv: row.dataset.credentialEnv,
          value: value,
        });
        const readiness = pluginReadiness(next, row.dataset.pluginId);
        const credential = (readiness.credentials || []).find(function (item) {
          return item.env_name === row.dataset.credentialEnv;
        });
        const missingReason = "CREDENTIAL_ENV_MISSING:" + row.dataset.credentialEnv;
        if (!credential || credential.source !== "saved" || readiness.reason_code === missingReason) {
          throw new Error(
            "The credential was written but did not become active in plugin readiness. Please retry or use Refresh.",
          );
        }
        pluginManagerState.tab = readiness.status === "ready" ? "enabled" : "needs_attention";
        succeeded = true;
        await refreshPluginManagerAfterMutation(next);
        showToast("API key saved on this device and readiness refreshed.");
      } catch (error) {
        showToast("API key save failed: " + String(error));
        if (input) input.focus();
      } finally {
        if (succeeded && input) input.value = "";
        if (button.isConnected) {
          button.disabled = false;
          button.textContent = originalLabel;
        }
      }
    };
  });

  document.querySelectorAll(".plugin-runtime-key-clear").forEach(function (button) {
    button.onclick = async function () {
      const row = button.closest(".plugin-runtime-credential");
      if (!row) return;
      const originalLabel = button.textContent;
      button.disabled = true;
      button.textContent = "Clearing…";
      try {
        const next = await call("clear_plugin_runtime_credential", {
          pluginId: row.dataset.pluginId,
          credentialEnv: row.dataset.credentialEnv,
        });
        const plugin = (next.plugins || []).find(function (item) {
          return item.id === row.dataset.pluginId;
        });
        pluginManagerState.tab = plugin ? pluginManagerBucket(next, plugin) : "needs_attention";
        await refreshPluginManagerAfterMutation(next);
        showToast("Saved API key cleared. Environment fallback was re-evaluated.");
      } catch (error) {
        showToast("Clearing the saved API key failed: " + String(error));
      } finally {
        if (button.isConnected) {
          button.disabled = false;
          button.textContent = originalLabel;
        }
      }
    };
  });
'''
replace_once(app_path, old_handlers, new_handlers, "credential handlers")

rust_path = "apps/desktop/src-tauri/src/application.rs"
replace_once(
    rust_path,
    '''    hydrate_persisted_plugin_credentials_v1(app, state)?;\n    let lifecycle = load_plugin_lifecycle_v1(app)?;\n    let runtime_credentials = runtime_plugin_credentials_snapshot_v1(state)?;\n''',
    '''    let mut runtime_credentials = runtime_plugin_credentials_snapshot_v1(state)?;\n    if runtime_credentials.is_empty() {\n        hydrate_persisted_plugin_credentials_v1(app, state)?;\n        runtime_credentials = runtime_plugin_credentials_snapshot_v1(state)?;\n    }\n    let lifecycle = load_plugin_lifecycle_v1(app)?;\n''',
    "avoid read-after-write credential reload",
)
replace_once(
    rust_path,
    '''    set_persisted_plugin_credential_v1(&app, &state, credential_env, value)?;\n    plugin_inventory_view_v1(&app, &state)\n}\n\n#[tauri::command]\nfn clear_plugin_runtime_credential(\n''',
    '''    set_persisted_plugin_credential_v1(&app, &state, credential_env, value)?;\n    let view = plugin_inventory_view_v1(&app, &state)?;\n    let readiness = view\n        .readiness\n        .iter()\n        .find(|item| item.plugin_id == plugin_id)\n        .ok_or_else(|| format!("Plugin {plugin_id} readiness was not projected after saving its API key."))?;\n    let credential = readiness\n        .credentials\n        .iter()\n        .find(|item| item.env_name == credential_env)\n        .ok_or_else(|| format!("Plugin {plugin_id} credential {credential_env} was not projected after save."))?;\n    if credential.source != "saved" {\n        return Err(format!(\n            "Plugin {plugin_id} credential {credential_env} did not become the saved credential source."\n        ));\n    }\n    let missing_reason = format!("CREDENTIAL_ENV_MISSING:{credential_env}");\n    if readiness.reason_code.as_deref() == Some(missing_reason.as_str()) {\n        return Err(format!(\n            "Plugin {plugin_id} still reports {missing_reason} after saving the credential."\n        ));\n    }\n    Ok(view)\n}\n\n#[tauri::command]\nfn clear_plugin_runtime_credential(\n''',
    "set credential postcondition",
)

# Strengthen static regression contract around the exact Windows error path.
test_path = Path("apps/desktop/tests/plugin-runtime-credentials-smoke.mjs")
test = test_path.read_text()
test = test.replace(
    'assert.match(app, /const next = await call\\("plugin_inventory"\\)/);\n',
    'assert.doesNotMatch(app, /set_plugin_runtime_credential[\\s\\S]{0,700}call\\("plugin_inventory"\\)/);\nassert.match(app, /const next = await call\\("set_plugin_runtime_credential"/);\nassert.match(app, /button\\.isConnected/);\nassert.match(app, /button\\.disabled = false/);\nassert.match(app, /if \\(succeeded && input\\) input\\.value = ""/);\nassert.match(app, /credential\\.source !== "saved"/);\n',
)
test = test.replace(
    'assert.match(rust, /hydrate_persisted_plugin_credentials_v1\\(app, state\\)/);\n',
    'assert.match(rust, /if runtime_credentials\\.is_empty\\(\\)/);\nassert.match(rust, /hydrate_persisted_plugin_credentials_v1\\(app, state\\)/);\nassert.match(rust, /did not become the saved credential source/);\n',
)
test_path.write_text(test)
