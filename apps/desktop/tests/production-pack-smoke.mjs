import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const appPath = path.join(root, "apps/desktop/dist/app.js");
const backendPath = path.join(root, "apps/desktop/src-tauri/src/main.rs");
const app = fs.readFileSync(appPath, "utf8");
const backend = fs.readFileSync(backendPath, "utf8");

const appMarkers = [
  "Export to Resolve",
  "Regenerate Production Pack",
  "export_production_pack",
  "production_export_status",
  "CANONICAL PRODUCTIONPACK V1 JSON",
  "LOGICAL PACKAGE LOCATION",
  "logical_uri",
  "Canonical export history",
];

for (const marker of appMarkers) {
  if (!app.includes(marker)) {
    throw new Error(`Production Pack desktop regression: missing app marker: ${marker}`);
  }
}

const backendMarkers = [
  "ProductionPackageExporterV1",
  "production_export_history_v1",
  "ProductionExportDiagnosticViewV1",
  '"missing_artifact"',
  "production_pack_logical_uri_v1",
];

for (const marker of backendMarkers) {
  if (!backend.includes(marker)) {
    throw new Error(`Production Pack desktop regression: missing backend marker: ${marker}`);
  }
}

const diagnosticStruct = backend.match(
  /struct ProductionExportDiagnosticViewV1 \{[\s\S]*?\n\}/,
);
if (!diagnosticStruct) {
  throw new Error("Production Pack desktop regression: diagnostic view is missing");
}
if (/\bpath\s*:/.test(diagnosticStruct[0])) {
  throw new Error(
    "Production Pack desktop regression: diagnostics must not serialize machine-specific physical paths",
  );
}

if (!backend.includes("CoreError::ExportArtifactFileMissing { artifact_id, .. }")) {
  throw new Error(
    "Production Pack desktop regression: missing-file diagnostics must discard the physical path",
  );
}

console.log("Production Pack desktop regression markers OK");
