import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const index = readFileSync(new URL("../dist/index.html", import.meta.url), "utf8");
const panel = readFileSync(
  new URL("../dist/omnivoice-studio-runtime.js", import.meta.url),
  "utf8",
);
const backend = readFileSync(
  new URL("../src-tauri/src/phase19_p5a.rs", import.meta.url),
  "utf8",
);
const main = readFileSync(new URL("../src-tauri/src/main.rs", import.meta.url), "utf8");

assert.match(index, /omnivoice-studio-runtime\.js/);
assert.match(panel, /save_omnivoice_studio_settings/);
assert.match(panel, /omnivoice_studio_status/);
assert.match(panel, /Save &amp; connect/);
assert.match(panel, /Machine-local runtime setting only/);
assert.match(panel, /Public Studio UI, Public REST API, or Public MCP URL/);

assert.match(backend, /app_config_dir\(\)/);
assert.match(backend, /omnivoice-studio\.json/);
assert.match(backend, /\/api\/v1\/capabilities/);
assert.match(backend, /\/api\/v1\/audio\/generate/);
assert.match(backend, /OMNIVOICE_API_TOKEN/);
assert.match(backend, /Do not embed credentials/);
assert.doesNotMatch(backend, /data_root.*omnivoice-studio\.json/i);
assert.doesNotMatch(backend, /bearer_token:\s*String/);

assert.match(main, /include!\("phase19_p5a\.rs"\)/);
assert.match(main, /run_phase19_p5a\(\)/);
