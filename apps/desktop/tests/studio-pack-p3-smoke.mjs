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
  path.join(root, "crates/omnicreator-core/src/project_board.rs"),
  "utf8",
);

const columns = [
  "IDEAS",
  "PREPARING",
  "NEEDS_REVIEW",
  "GPU_READY",
  "GPU_RUNNING",
  "READY_TO_EDIT",
  "DONE",
];

for (const column of columns) {
  if (!app.includes(`id: "${column}"`)) {
    throw new Error(`P3 Kanban regression: missing board column ${column}`);
  }
}

const appMarkers = [
  "renderProjectKanban",
  "project-action-summary",
  "Review Issues",
  "studio-pack-creator",
  "review-center",
  "gpu-workbench",
  "prepare-gpu-batch",
  "production-pack-panel",
  "Export to Resolve",
  "Read-only mode. Project state is visible",
];

for (const marker of appMarkers) {
  if (!app.includes(marker)) {
    throw new Error(`P3 Kanban regression: missing app marker: ${marker}`);
  }
}

const backendMarkers = [
  "ProjectBoardProjectionV1",
  "project_board_projection_v1",
  "prepare_writable_session",
  "reconcile_interrupted_jobs",
  "StateStore::open_read_only",
];

for (const marker of backendMarkers) {
  if (!backend.includes(marker)) {
    throw new Error(`P3 Kanban regression: missing backend marker: ${marker}`);
  }
}

const coreMarkers = [
  "ProjectDisplayStatus::GpuPartial",
  "ProjectBoardColumnV1::NeedsReview",
  "board_reconstructs_after_data_root_move_in_read_only_mode",
  "interrupted_running_job_reconciles_into_review_column",
];

for (const marker of coreMarkers) {
  if (!core.includes(marker)) {
    throw new Error(`P3 Kanban regression: missing core marker: ${marker}`);
  }
}

if (/localStorage|sessionStorage/.test(app)) {
  throw new Error(
    "P3 Kanban regression: project-board placement must not become browser durable state",
  );
}

if (/kanban[_-](status|state).*UPDATE|UPDATE.*kanban/i.test(backend + core)) {
  throw new Error(
    "P3 Kanban regression: Kanban must remain a projection, not a persisted workflow status",
  );
}

console.log("Studio Pack P3 Kanban/hardening regression markers OK");
