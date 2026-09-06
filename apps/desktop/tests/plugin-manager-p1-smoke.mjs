import fs from "node:fs";

const main = fs.readFileSync("apps/desktop/src-tauri/src/main.rs", "utf8");
const lifecycle = fs.readFileSync(
  "crates/omnicreator-core/src/plugin_lifecycle.rs",
  "utf8",
);

for (const marker of [
  "install_plugin_from_folder",
  "uninstall_plugin",
  "install_local_plugin_folder_v1",
  "uninstall_user_plugin_v1",
  "plugin_user_root_v1",
]) {
  if (!main.includes(marker)) {
    throw new Error(`desktop Plugin Manager P1 marker missing: ${marker}`);
  }
}

for (const marker of [
  ".install-staging",
  ".uninstall-staging",
  "symlink_metadata",
  "copy_plugin_directory_v1",
  "scan_plugin_roots",
  "fs::rename",
  "plugin_install_directory_name_v1",
]) {
  if (!lifecycle.includes(marker)) {
    throw new Error(`safe local plugin lifecycle marker missing: ${marker}`);
  }
}

if (lifecycle.includes("std::process::Command") || lifecycle.includes("Command::new")) {
  throw new Error("package inspection must not execute plugin commands");
}

if (!main.includes("app_data_dir()") || !main.includes("app_config_dir()")) {
  throw new Error("plugin installation/lifecycle state must remain machine-local");
}

console.log("Plugin Manager P1 smoke markers are present.");
