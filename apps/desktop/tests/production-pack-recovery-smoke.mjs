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
  "Production Recovery",
  "Repair / Relink ",
  "production-repair-visual",
  "production-repair-audio",
  "production-repair-timing",
  "Repair Audio + Timing",
  "Rebuild + Regenerate Resolve",
  "production_recovery_status",
  "repair_production_visual",
  "repair_production_audio",
  "repair_production_timing",
  "repair_production_voice_bundle",
  "rebuild_recovered_production",
];

const backendMarkers = [
  "inspect_creator_production_recovery_v1",
  "repair_creator_production_visual_v1",
  "repair_creator_production_audio_v1",
  "repair_creator_production_timing_v1",
  "repair_creator_production_voice_bundle_v1",
  "rebuild_and_export_creator_production_v1",
  "readable_store(&state)",
  "writable_store(&state)",
];

for (const marker of appMarkers) {
  if (!app.includes(marker)) {
    throw new Error(`Production recovery desktop regression: missing app marker: ${marker}`);
  }
}
for (const marker of backendMarkers) {
  if (!backend.includes(marker)) {
    throw new Error(`Production recovery desktop regression: missing backend marker: ${marker}`);
  }
}
if (app.includes("CANONICAL PRODUCTIONPACK V1 JSON") || app.includes("productionPack: parsed")) {
  throw new Error("Production recovery must not require hand-edited ProductionPack JSON");
}
if (!app.includes("Read-only: inspection only. Relink and rebuild mutations are disabled.")) {
  throw new Error("Production recovery must communicate read-only inspection semantics");
}
console.log("Production recovery desktop regression markers OK");
