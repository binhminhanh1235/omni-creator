import assert from "node:assert/strict";
import fs from "node:fs";

const app = fs.readFileSync(new URL("../dist/app.js", import.meta.url), "utf8");
const rust = fs.readFileSync(new URL("../src-tauri/src/application.rs", import.meta.url), "utf8");
const core = fs.readFileSync(new URL("../../../crates/omnicreator-core/src/plugin_process.rs", import.meta.url), "utf8");
const credentials = fs.readFileSync(new URL("../src-tauri/src/plugin_credentials.rs", import.meta.url), "utf8");

assert.match(app, /type=\"password\"/);
assert.match(app, /set_plugin_runtime_credential/);
assert.match(app, /clear_plugin_runtime_credential/);
assert.match(app, /Saved on this device outside the Data Root/);
assert.doesNotMatch(app, /localStorage[^\n]*runtime_plugin/i);
assert.match(rust, /runtime_plugin_credentials: Mutex<BTreeMap<String, String>>/);
assert.match(rust, /if runtime_credentials\.is_empty\(\)/);
assert.match(rust, /hydrate_persisted_plugin_credentials_v1\(app, state\)/);
assert.match(rust, /did not become the saved credential source/);
assert.match(rust, /validate_plugin_runtime_credential_target_v1/);
assert.match(rust, /runtime_credential_source_v1/);
assert.doesNotMatch(app, /set_plugin_runtime_credential[\s\S]{0,700}call\("plugin_inventory"\)/);
assert.match(app, /const next = await call\("set_plugin_runtime_credential"/);
assert.match(app, /button\.isConnected/);
assert.match(app, /button\.disabled = false/);
assert.match(app, /if \(succeeded && input\) input\.value = ""/);
assert.match(app, /credential\.source !== "saved"/);
assert.match(app, /Saved key active/);
assert.match(app, /Replace saved key/);
assert.match(rust, /spawn_with_env\(plugin, PluginProcessOptions::default\(\), &environment\)/);
assert.match(core, /pub fn spawn_with_env/);
assert.match(core, /\.envs\(environment\)/);
assert.match(credentials, /plugin-credentials\.json/);
assert.match(credentials, /save_persisted_plugin_credentials_v1/);
assert.match(credentials, /set_persisted_plugin_credential_v1/);
assert.match(credentials, /clear_persisted_plugin_credential_v1/);
assert.doesNotMatch(app, /first-secret|replacement-secret/);

console.log("runtime plugin credential smoke: ok");
