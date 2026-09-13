import fs from "node:fs";
import assert from "node:assert/strict";

const ui = fs.readFileSync(new URL("../dist/workflow-step-controls.js", import.meta.url), "utf8");
const index = fs.readFileSync(new URL("../dist/index.html", import.meta.url), "utf8");
const main = fs.readFileSync(new URL("../src-tauri/src/main.rs", import.meta.url), "utf8");
const phase17 = fs.readFileSync(new URL("../src-tauri/src/phase17.rs", import.meta.url), "utf8");

new Function(ui);

for (const key of [
  "content.prepare",
  "scene.plan",
  "visual.prepare",
  "voice.prepare",
  "production.pack",
]) {
  assert.ok(ui.includes(key), `missing UI step key ${key}`);
}

assert.ok(ui.includes("workflow_step_execution_policies"));
assert.ok(ui.includes("set_workflow_step_automatic_execution"));
assert.ok(ui.includes("start_creator_production_phase17"));
assert.ok(ui.includes("AUTO ON"));
assert.ok(ui.includes("AUTO OFF"));
assert.ok(ui.includes("manual/external"));

const controlsIndex = index.indexOf("workflow-step-controls.js");
const appIndex = index.indexOf("app.js");
assert.ok(controlsIndex >= 0 && appIndex >= 0 && controlsIndex < appIndex, "invoke shim must load before app.js");
assert.ok(index.includes("workflow-step-controls.css"));

assert.ok(main.includes('include!("application.rs")'));
assert.ok(main.includes('include!("phase17.rs")'));
assert.ok(phase17.includes("start_creator_production_phase17"));
assert.ok(phase17.includes("workflow_step_auto_enabled_phase17"));
assert.ok(phase17.includes("CREATOR_STEP_CONTENT_PREPARE_V1"));
assert.ok(phase17.includes("CREATOR_STEP_SCENE_PLAN_V1"));
assert.ok(phase17.includes("CREATOR_STEP_VISUAL_PREPARE_V1"));
assert.ok(phase17.includes("CREATOR_STEP_VOICE_PREPARE_V1"));
assert.ok(phase17.includes("CREATOR_STEP_PRODUCTION_PACK_V1"));

console.log("workflow step controls smoke: ok");
