import fs from "node:fs";

const app = fs.readFileSync("apps/desktop/dist/app.js", "utf8");
const main = fs.readFileSync("apps/desktop/src-tauri/src/main.rs", "utf8");
const styles = fs.readFileSync("apps/desktop/dist/styles.css", "utf8");

for (const marker of [
  "plugin-manager-panel",
  "Installed / Enabled",
  "Disabled",
  "Needs Attention",
  "Install from Local Folder",
  "plugin_inventory",
  "set_plugin_enabled",
  "install_plugin_from_folder",
  "uninstall_plugin",
  "inspect_plugin_update",
  "apply_plugin_update",
  "plugin_mutation_impact",
  "preparePluginMutation",
  "plugin-impact-preview",
  "capability_delta",
  "blocking_pack_ids",
  "blocking_project_ids",
  "Data Root is read-only",
  "loadStudioPackWorkspace",
]) {
  if (!app.includes(marker)) {
    throw new Error(`desktop Plugin Manager P3 UX marker missing: ${marker}`);
  }
}

for (const marker of [
  "pick_plugin_folder",
  "PluginRuntimeReadinessDesktopViewV1",
  "studio_pack_runtime_snapshot_v1",
  "PluginRuntimeReadinessV1::SetupRequired",
  "PluginRuntimeReadinessV1::Unavailable",
]) {
  if (!main.includes(marker)) {
    throw new Error(`desktop Plugin Manager P3 backend marker missing: ${marker}`);
  }
}

for (const marker of [
  ".plugin-manager-panel",
  ".plugin-grid",
  ".plugin-card",
  ".plugin-tabs",
  ".plugin-impact-preview",
]) {
  if (!styles.includes(marker)) {
    throw new Error(`desktop Plugin Manager P3 style marker missing: ${marker}`);
  }
}

const disablePreview = app.indexOf('preparePluginMutation("disable"');
const removePreview = app.indexOf('preparePluginMutation("remove"');
const directDisable = app.indexOf('enabled: false');
const directRemove = app.indexOf('call("uninstall_plugin"');
if (disablePreview < 0 || removePreview < 0 || directDisable < 0 || directRemove < 0) {
  throw new Error("destructive lifecycle mutations must keep impact preview gates");
}
if (disablePreview > directDisable || removePreview > directRemove) {
  throw new Error("impact preview wiring must be declared before destructive mutation application");
}

if (app.includes("marketplace") || app.includes("background automatic update")) {
  throw new Error("P3 must not introduce marketplace or background-update scope");
}

if (!app.includes("Project and Studio Pack portable state is not rewritten")) {
  throw new Error("P3 must preserve the portable Project/StudioPack boundary");
}

console.log("Plugin Manager P3 smoke markers are present.");
