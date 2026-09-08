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
  path.join(root, "crates/omnicreator-core/src/creator_external_handoff.rs"),
  "utf8",
);

const appMarkers = [
  "EXTERNAL GENERATED HANDOFF",
  "EXTERNAL VOICE / COMPUTE HANDOFF",
  "Copy Request",
  "Export Request",
  "Provide External Result",
  "Replace External Result",
  "<summary>Details</summary>",
  "creator-copy-external-request",
  "creator-export-external-request",
  "creator-provide-external-visual",
  "creator-provide-external-voice",
  "export_creator_external_visual_request",
  "provide_creator_external_visual_result",
  "export_creator_external_voice_request",
  "provide_creator_external_voice_result",
  "request: request",
];

const backendMarkers = [
  "prepare_external_generated_visual_request_v1",
  "prepare_external_voice_request_v1",
  "provide_external_generated_visual_result_v1",
  "provide_external_voice_result_v1",
  "export_creator_external_visual_request",
  "provide_creator_external_visual_result",
  "export_creator_external_voice_request",
  "provide_creator_external_voice_result",
  "readable_store(&state)",
  "writable_store(&state)",
  "request.validate_v1().map_err(error_string)?",
  "External visual request identity does not match the selected project/scene.",
  "External voice request identity does not match the selected project/segment.",
];

const coreMarkers = [
  "ExternalGeneratedVisualRequestV1",
  "ExternalVoiceRequestV1",
  "ManualResultProvenanceV1::external",
  "provide_manual_creator_visual_file_v1",
  "provide_manual_creator_voice_bundle_v1",
  "inspect_manual_visual_media_v1",
  "request_sha256",
  "external generated visual request is stale",
  "external voice request is stale",
];

for (const marker of appMarkers) {
  if (!app.includes(marker)) {
    throw new Error("External handoff desktop regression: missing app marker: " + marker);
  }
}
for (const marker of backendMarkers) {
  if (!backend.includes(marker)) {
    throw new Error("External handoff desktop regression: missing backend marker: " + marker);
  }
}
for (const marker of coreMarkers) {
  if (!core.includes(marker)) {
    throw new Error("External handoff core regression: missing marker: " + marker);
  }
}

function buttonFragment(className) {
  const start = app.indexOf('class="btn ' + className + '"');
  if (start < 0) throw new Error("Missing button class: " + className);
  const end = app.indexOf("</button>", start);
  return app.slice(start, end);
}

for (const className of [
  "creator-provide-external-visual",
  "creator-provide-external-voice",
]) {
  const fragment = buttonFragment(className);
  if (!fragment.includes("readOnly") || !fragment.includes("disabled")) {
    throw new Error(
      "External handoff read-only regression: import/replace must be disabled for " + className,
    );
  }
}

for (const className of [
  "creator-copy-external-request",
  "creator-export-external-request",
]) {
  const fragment = buttonFragment(className);
  if (fragment.includes("readOnly") && fragment.includes("disabled")) {
    throw new Error(
      "External handoff read-only regression: inspect/copy/export must remain available for " +
        className,
    );
  }
}

if (app.includes("localStorage") || app.includes("sessionStorage")) {
  throw new Error(
    "External handoff desktop regression: browser storage must not become workflow state",
  );
}

if (!backend.includes("fn export_creator_external_visual_request") ||
    !backend.includes("let store = readable_store(&state)?;")) {
  throw new Error("External request export must be derived from readable canonical state");
}

console.log("Phase 16 P4 external handoff desktop regression markers OK");
