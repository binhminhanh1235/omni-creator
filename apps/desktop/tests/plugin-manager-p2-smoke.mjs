import fs from "node:fs";

const main = fs.readFileSync("apps/desktop/src-tauri/src/main.rs", "utf8");
const lifecycle = fs.readFileSync(
  "crates/omnicreator-core/src/plugin_lifecycle.rs",
  "utf8",
);
const cargo = fs.readFileSync("crates/omnicreator-core/Cargo.toml", "utf8");

for (const marker of [
  "inspect_plugin_update",
  "apply_plugin_update",
  "plugin_mutation_impact",
  "plugin_impact_context_v1",
  "inspect_local_plugin_update_v1",
  "update_local_plugin_folder_v1",
  "preview_plugin_capability_impact_v1",
]) {
  if (!main.includes(marker)) {
    throw new Error(`desktop Plugin Manager P2 marker missing: ${marker}`);
  }
}

for (const marker of [
  "PluginUpdatePreviewV1",
  "PluginCapabilityDeltaV1",
  "PluginCapabilityImpactV1",
  "PluginMutationKindV1",
  ".update-staging",
  "activate_plugin_update_v1",
  "failed-candidate",
  "Version::parse",
]) {
  if (!lifecycle.includes(marker)) {
    throw new Error(`Plugin Manager P2 core marker missing: ${marker}`);
  }
}

if (!cargo.includes('semver = "1"')) {
  throw new Error("P2 semantic version compatibility gate dependency is missing");
}

if (lifecycle.includes("std::process::Command") || lifecycle.includes("Command::new")) {
  throw new Error("update inspection must not execute plugin commands");
}

console.log("Plugin Manager P2 smoke markers are present.");
