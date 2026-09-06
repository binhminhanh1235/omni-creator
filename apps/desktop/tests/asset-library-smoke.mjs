import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const app = fs.readFileSync(path.join(root, "apps/desktop/dist/app.js"), "utf8");
const backend = fs.readFileSync(
  path.join(root, "apps/desktop/src-tauri/src/main.rs"),
  "utf8",
);
const core = fs.readFileSync(
  path.join(root, "crates/omnicreator-core/src/asset_library.rs"),
  "utf8",
);
const state = fs.readFileSync(
  path.join(root, "crates/omnicreator-core/src/state.rs"),
  "utf8",
);

const appMarkers = [
  "asset-library-panel",
  "renderAssetLibrary",
  "loadAssetLibrary",
  "Reuse intelligence",
  "Exact duplicates",
  "Source reused",
  "add_asset_tag",
  "remove_asset_tag",
  "loadAssetLibrary(workspace.read_only)",
];

for (const marker of appMarkers) {
  if (!app.includes(marker)) {
    throw new Error(`Asset Library regression: missing app marker: ${marker}`);
  }
}

const backendMarkers = [
  "fn asset_library",
  "fn add_asset_tag",
  "fn remove_asset_tag",
  "writable_store(&state)?",
  "asset_library_snapshot_v1",
];

for (const marker of backendMarkers) {
  if (!backend.includes(marker)) {
    throw new Error(`Asset Library regression: missing backend marker: ${marker}`);
  }
}

const coreMarkers = [
  "replace_asset_tags_v1",
  "record_asset_usage_v1",
  "source_reuse_facts_v1",
  "duplicate_groups",
  "source_reuse_groups",
  "used_recently",
  "library_projection_survives_data_root_move_and_read_only_open",
  "idx_artifacts_sha256",
];

for (const marker of coreMarkers) {
  if (!core.includes(marker) && !state.includes(marker)) {
    throw new Error(`Asset Library regression: missing core marker: ${marker}`);
  }
}

const migrationMarkers = [
  "MIGRATION_V8",
  "artifact_tags",
  "artifact_usages",
  "idx_artifacts_sha256",
];

for (const marker of migrationMarkers) {
  if (!state.includes(marker)) {
    throw new Error(`Asset Library regression: missing migration marker: ${marker}`);
  }
}

if (/localStorage|sessionStorage/.test(app)) {
  throw new Error(
    "Asset Library regression: library intelligence must not become browser durable state",
  );
}

if (/embedding|vector[_ -]?db|visual[_ -]?similarity/i.test(core + state + backend)) {
  throw new Error(
    "Asset Library regression: embeddings/vector similarity are explicitly deferred from Phase 12",
  );
}

console.log("Phase 12 Asset Library intelligence regression markers OK");
