import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";

const index = readFileSync("apps/desktop/dist/index.html", "utf8");
const settings = readFileSync("apps/desktop/dist/unified-settings.js", "utf8");
const application = readFileSync("apps/desktop/src-tauri/src/application.rs", "utf8");
const lifecycle = readFileSync("crates/omnicreator-core/src/plugin_lifecycle.rs", "utf8");
const tauri = JSON.parse(readFileSync("apps/desktop/src-tauri/tauri.conf.json", "utf8"));

assert.match(index, /omnivoice-studio-runtime\.js/);
assert.match(index, /unified-settings\.js/);
assert.ok(
  index.indexOf("omnivoice-studio-runtime.js") < index.indexOf("unified-settings.js"),
  "unified Settings must install after the existing runtime surfaces it consolidates",
);

for (const marker of [
  "unified-settings-button",
  "unified-settings-modal",
  "Workspace",
  "AI &amp; Runtime",
  "Plugins",
  "llmgateway-panel",
  "plugin-manager-panel",
  "omnivoice-studio-runtime-panel",
  "review-configure-llm",
  "open-plugin-manager",
  "Machine-local LLM Provider and OmniVoiceStudio endpoints",
  "Secret values remain outside the Data Root",
  "Built-in plugins ship with OmniCreator",
]) {
  assert.ok(settings.includes(marker), `Phase 20 unified Settings marker missing: ${marker}`);
}

assert.match(settings, /#omnivoice-studio-runtime-button \{ display: none !important; \}/);
assert.match(settings, /p20-settings-source-stack/);
assert.match(settings, /p20-settings-detached/);
assert.match(settings, /MutationObserver/);
assert.match(settings, /source\.click\(\)/);

assert.equal(tauri.bundle.active, true, "Desktop bundling must be active for bundled plugin resources");
assert.equal(
  tauri.bundle.resources["../../../plugins/"],
  "plugins/",
  "checked-in plugins must be copied to the resource_dir/plugins root expected by runtime discovery",
);
assert.match(application, /resource_dir\(\)/);
assert.match(application, /resources\.join\("plugins"\)/);
assert.match(lifecycle, /disabled_plugin_ids:\s*BTreeSet::new\(\)/);

for (const plugin of [
  "generated-image-api",
  "generated-image-reference",
  "pexels",
  "pixabay",
  "stick-figure-reference",
  "storyblocks",
  "unsplash",
]) {
  assert.ok(existsSync(`plugins/${plugin}/plugin.yaml`), `built-in plugin manifest missing: ${plugin}`);
}

assert.doesNotMatch(settings, /API[_ -]?KEY\s*=\s*["'][^"']+["']/i);
assert.doesNotMatch(settings, /bearer[_ -]?token\s*=\s*["'][^"']+["']/i);

console.log("Phase 20 unified Settings and bundled built-in plugin smoke markers are present.");
