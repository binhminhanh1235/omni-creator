import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const app = fs.readFileSync(path.join(root, "apps/desktop/dist/app.js"), "utf8");
const backend = fs.readFileSync(
  path.join(root, "apps/desktop/src-tauri/src/main.rs"),
  "utf8",
);

const appMarkers = [
  "MANUAL VISUAL TAKEOVER",
  "Choose From Asset Library",
  "Use My Image",
  "Use My Video",
  "Replace Existing Visual",
  "creator_manual_visual_status",
  "choose_creator_asset_library_visual",
  "use_creator_image_manually",
  "use_creator_video_manually",
];

const backendMarkers = [
  "creator_scene_visual_states_v1",
  "provide_manual_creator_visual_file_v1",
  "choose_creator_visual_from_asset_library_v1",
  "CreatorManualVisualDesktopViewV1",
  "creator_manual_visual_status",
  "use_creator_image_manually",
  "use_creator_video_manually",
  "choose_creator_asset_library_visual",
];

for (const marker of appMarkers) {
  if (!app.includes(marker)) {
    throw new Error("Manual visual desktop regression: missing app marker: " + marker);
  }
}
for (const marker of backendMarkers) {
  if (!backend.includes(marker)) {
    throw new Error("Manual visual desktop regression: missing backend marker: " + marker);
  }
}

if (app.includes("localStorage") || app.includes("sessionStorage")) {
  throw new Error(
    "Manual visual desktop regression: browser storage must not become canonical workflow state",
  );
}

console.log("Manual visual takeover desktop regression markers OK");
