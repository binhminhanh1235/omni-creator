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
  path.join(root, "crates/omnicreator-core/src/creator_manual_voice.rs"),
  "utf8",
);

const appMarkers = [
  "MANUAL VOICE + TIMING TAKEOVER",
  "Use My WAV",
  "Audio + Timing",
  "Replace Timing",
  "creator_manual_voice_status",
  "use_creator_wav_manually",
  "use_creator_audio_with_timing_manually",
  "replace_creator_voice_timing_manually",
];

const backendMarkers = [
  "creator_segment_voice_states_v1",
  "derive_manual_voice_timing_v1",
  "import_manual_voice_timing_file_v1",
  "provide_manual_creator_voice_bundle_v1",
  "replace_manual_creator_voice_timing_v1",
  "CreatorManualVoiceDesktopViewV1",
  "creator_manual_voice_status",
  "use_creator_wav_manually",
  "use_creator_audio_with_timing_manually",
  "replace_creator_voice_timing_manually",
];

const coreMarkers = [
  "ManualCreatorVoiceRequestV1",
  "LocalVoiceBundlePromotionV1",
  "promote_local_voice_bundle_v1",
  "commit_voice_bundle_success_v1",
  "parse_manual_voice_timing_bytes_v1",
  "voice_timing_to_srt_v1",
  "reconcile_creator_voice_aggregate_v1",
];

for (const marker of appMarkers) {
  if (!app.includes(marker)) {
    throw new Error("Manual voice desktop regression: missing app marker: " + marker);
  }
}
for (const marker of backendMarkers) {
  if (!backend.includes(marker)) {
    throw new Error("Manual voice desktop regression: missing backend marker: " + marker);
  }
}
for (const marker of coreMarkers) {
  if (!core.includes(marker)) {
    throw new Error("Manual voice desktop regression: missing core marker: " + marker);
  }
}

if (app.includes("localStorage") || app.includes("sessionStorage")) {
  throw new Error(
    "Manual voice desktop regression: browser storage must not become canonical workflow state",
  );
}
if (core.includes("source_path") || core.includes("absolute_path")) {
  throw new Error(
    "Manual voice desktop regression: canonical voice metadata must not persist absolute source paths",
  );
}

console.log("Manual voice + timing takeover desktop regression markers OK");
