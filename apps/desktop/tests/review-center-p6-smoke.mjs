import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const index = fs.readFileSync(path.join(root, "apps/desktop/dist/index.html"), "utf8");
const ui = fs.readFileSync(path.join(root, "apps/desktop/dist/review-center-p6.js"), "utf8");
const core = fs.readFileSync(
  path.join(root, "crates/omnicreator-core/src/review_center_recovery.rs"),
  "utf8",
);
const backend = fs.readFileSync(
  path.join(root, "apps/desktop/src-tauri/src/main.rs"),
  "utf8",
);

for (const asset of ["review-center-p6.css", "review-center-p6.js"]) {
  if (!index.includes(asset)) {
    throw new Error(`P6 Review Center regression: index does not load ${asset}`);
  }
}

const uiMarkers = [
  "Universal recovery",
  "provide_manually",
  "replace_result",
  "choose_alternative",
  "external_handoff",
  "production_recovery_status",
  "retry_review_job",
  "Read-only workspace",
  "NEXT VALID ACTIONS",
  "explicitAlternativeChoice: false",
];
for (const marker of uiMarkers) {
  if (!ui.includes(marker)) {
    throw new Error(`P6 Review Center regression: missing UI marker ${marker}`);
  }
}

const coreMarkers = [
  "ReviewRecoveryActionKindV1",
  "ReviewRecoveryBlockerClassV1",
  "ReviewRecoveryActionTargetV1",
  "derive_review_recovery_actions_v1",
  "explicit_alternative_choice",
  "read_only_keeps_details_and_suppresses_mutation_execution",
  "serialized_action_payload_has_no_secret_or_machine_path_fields",
  "deterministic_fallback_does_not_pretend_user_choice_exists",
];
for (const marker of coreMarkers) {
  if (!core.includes(marker)) {
    throw new Error(`P6 Review Center regression: missing core marker ${marker}`);
  }
}

const canonicalControllerMarkers = [
  "retry_review_job",
  "provide_creator_script_manually",
  "provide_creator_scene_plan_manually",
  "use_creator_image_manually",
  "use_creator_video_manually",
  "export_creator_external_visual_request",
  "provide_creator_external_visual_result",
  "use_creator_wav_manually",
  "use_creator_audio_with_timing_manually",
  "export_creator_external_voice_request",
  "provide_creator_external_voice_result",
  "production_recovery_status",
  "repair_production_visual",
  "repair_production_audio",
  "repair_production_timing",
  "rebuild_recovered_production",
];
for (const marker of canonicalControllerMarkers) {
  if (!backend.includes(marker)) {
    throw new Error(`P6 Review Center regression: canonical controller missing ${marker}`);
  }
}

if (/localStorage|sessionStorage/.test(ui)) {
  throw new Error("P6 Review Center regression: browser durable state is forbidden");
}
if (/UPDATE\s+|INSERT\s+INTO|DELETE\s+FROM/i.test(ui)) {
  throw new Error("P6 Review Center regression: UI must not contain direct SQL mutation");
}
if (/bearer\s+|authorization\s*:/i.test(ui)) {
  throw new Error("P6 Review Center regression: action/details UI must not carry auth material");
}

console.log("Phase 16 P6 Review Center universal recovery regression markers OK");
