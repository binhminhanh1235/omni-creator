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
  path.join(root, "crates/omnicreator-core/src/studio_pack_ux.rs"),
  "utf8",
);
const catalog = fs.readFileSync(
  path.join(root, "crates/omnicreator-core/src/studio_pack_catalog.rs"),
  "utf8",
);

const appMarkers = [
  "Start with a Studio Pack",
  "create_project_from_studio_pack",
  "update_project_studio_pack",
  "studio_pack_catalog",
  "review_center",
  "retry_review_job",
  "Resolved plugin / capability routing",
  "Exceptions, not busywork",
  "Plugin wiring stays out of the Basic flow",
  "Provider endpoints, credentials and absolute paths are never written into the portable Studio Pack",
];

for (const marker of appMarkers) {
  if (!app.includes(marker)) {
    throw new Error(`Studio Pack P2 regression: missing app marker: ${marker}`);
  }
}

const backendMarkers = [
  "build_studio_pack_ux_view_v1",
  "build_studio_review_center_v1",
  "scan_plugin_roots",
  "load_plugin_settings_ui",
  "PluginRuntimeReadinessV1::SetupRequired",
  ".omnicreator/studio-pack-catalog.json",
  "create_project_with_studio_pack",
  "prepare_job_retry",
];

for (const marker of backendMarkers) {
  if (!backend.includes(marker)) {
    throw new Error(`Studio Pack P2 regression: missing backend marker: ${marker}`);
  }
}

const coreMarkers = [
  "StudioAutomationLevelV1::Assisted",
  "StudioAutomationLevelV1::Balanced",
  "StudioAutomationLevelV1::Autopilot",
  "stop_on_blocking_exception: true",
  "canonical_source: \"Job / Attempt\"",
  "canonical_source: \"WorkflowStep\"",
  "PluginRegistry + StudioPack availability",
];

for (const marker of coreMarkers) {
  if (!core.includes(marker)) {
    throw new Error(`Studio Pack P2 regression: missing core marker: ${marker}`);
  }
}

if (/localStorage|sessionStorage/.test(app)) {
  throw new Error(
    "Studio Pack P2 regression: creator/review state must not become browser durable truth",
  );
}

if (/review-center\.json|review_center\.db/i.test(app + backend + core)) {
  throw new Error(
    "Studio Pack P2 regression: Review Center must not introduce a durable shadow store",
  );
}

if (!catalog.includes("STICK_FIGURE_VISUAL_CAPABILITY_V1")) {
  throw new Error(
    "Studio Pack P2 regression: Christian Stick Explainer semantic capability gate disappeared",
  );
}

console.log("Studio Pack P2 desktop regression markers OK");
