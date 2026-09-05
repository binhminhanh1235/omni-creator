import fs from "node:fs";

const source = fs.readFileSync(new URL("../dist/app.js", import.meta.url), "utf8");

const required = [
  "project-batch-checkbox",
  "Prepare GPU Batch",
  "connect_compute_provider",
  "gpu_workbench_review",
  "start_gpu_burst",
  "sync_compute_burst",
  "execution_specs",
  "Advanced / provider runtime details",
  "RUNNING",
  "COMPLETED",
  "REMAINING",
  "RETRYABLE",
  "no VRAM pooling",
];

for (const marker of required) {
  if (!source.includes(marker)) {
    throw new Error("GPU Workbench regression: missing " + marker);
  }
}

for (const forbidden of [/\bkaggle\b/i, /\bredis\b/i, /\bkafka\b/i, /\brabbitmq\b/i, /\bkubernetes\b/i]) {
  if (forbidden.test(source)) {
    throw new Error("GPU Workbench provider-neutral regression: forbidden desktop token " + forbidden);
  }
}

console.log("GPU Workbench desktop regression markers: PASS");
