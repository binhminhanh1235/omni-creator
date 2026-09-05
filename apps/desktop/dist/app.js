const invoke = window.__TAURI__.core.invoke;
const content = document.getElementById("content");
const modePill = document.getElementById("mode-pill");
const toast = document.getElementById("toast");
const gpuWorkbenchState = {
  selectedProjectIds: new Set(),
  context: { preparations: [], providers: [], running: [], execution_specs: [] },
  provider: null,
  review: null,
  syncTimer: null,
};

function escapeHtml(value) {
  return String(value == null ? "" : value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function showToast(message) {
  toast.textContent = String(message);
  toast.hidden = false;
  clearTimeout(showToast.timer);
  showToast.timer = setTimeout(function () {
    toast.hidden = true;
  }, 5200);
}

async function call(command, args) {
  try {
    return await invoke(command, args || {});
  } catch (error) {
    showToast(error);
    throw error;
  }
}

function setMode(text, className) {
  modePill.textContent = text;
  modePill.className = ("pill " + (className || "")).trim();
}

function render(snapshot) {
  switch (snapshot.kind) {
    case "unconfigured":
      renderFirstLaunch();
      break;
    case "conflict":
      renderConflict(snapshot);
      break;
    case "unavailable":
      renderUnavailable(snapshot);
      break;
    case "ready":
      renderWorkspace(snapshot);
      break;
    case "handoff_ready":
      renderHandoff(snapshot);
      break;
    default:
      renderUnavailable({
        data_root: "",
        message: "Unknown desktop state: " + snapshot.kind,
      });
  }
}

function renderFirstLaunch() {
  setMode("SETUP");
  content.innerHTML =
    '<section class="hero-card">' +
    '<p class="eyebrow">FIRST LAUNCH</p>' +
    '<h1>Where should OmniCreator keep your data?</h1>' +
    '<p class="muted">All projects, media and production state will live here. You can move or sync this folder later.</p>' +
    '<div class="actions">' +
    '<button class="btn primary" id="create-root">Create New Data Folder</button>' +
    '<button class="btn" id="existing-root">Use Existing Data Folder</button>' +
    "</div></section>";

  document.getElementById("create-root").onclick = function () {
    chooseRoot("create_data_root");
  };
  document.getElementById("existing-root").onclick = function () {
    chooseRoot("use_existing_data_root");
  };
}

async function chooseRoot(command) {
  const dataRoot = await call("pick_data_root");
  if (!dataRoot) return;
  render(await call(command, { dataRoot: dataRoot }));
}

function renderConflict(snapshot) {
  setMode("CONFLICT", "read-only");
  content.innerHTML =
    '<section class="hero-card">' +
    '<p class="eyebrow">WORKSPACE SAFETY</p>' +
    '<h1>This workspace may still be open elsewhere.</h1>' +
    '<p class="muted">' + escapeHtml(snapshot.message) + "</p>" +
    '<div class="notice">OmniCreator will not automatically open a second writer against a synchronized Data Folder.</div>' +
    '<div class="info-row"><div class="info-label">DATA FOLDER</div><div class="info-value">' +
    escapeHtml(snapshot.data_root) +
    "</div></div>" +
    '<div class="actions">' +
    '<button class="btn primary" id="read-only">Open Read Only</button>' +
    '<button class="btn" id="check-again">Check Again</button>' +
    '<button class="btn" id="different-root">Choose Different Folder</button>' +
    "</div></section>";

  document.getElementById("read-only").onclick = async function () {
    render(await call("open_read_only", { dataRoot: snapshot.data_root }));
  };
  document.getElementById("check-again").onclick = async function () {
    render(await call("check_again", { dataRoot: snapshot.data_root }));
  };
  document.getElementById("different-root").onclick = renderFirstLaunch;
}

function renderUnavailable(snapshot) {
  setMode("ACTION NEEDED", "read-only");
  const configured = snapshot.data_root
    ? '<div class="info-row"><div class="info-label">CONFIGURED DATA FOLDER</div><div class="info-value">' +
      escapeHtml(snapshot.data_root) +
      "</div></div>"
    : "";

  content.innerHTML =
    '<section class="hero-card">' +
    '<p class="eyebrow">DATA FOLDER</p>' +
    '<h1>OmniCreator cannot open this workspace yet.</h1>' +
    '<p class="muted">' + escapeHtml(snapshot.message) + "</p>" +
    configured +
    '<div class="actions">' +
    '<button class="btn primary" id="select-existing">Use Existing Data Folder</button>' +
    '<button class="btn" id="select-new">Create New Data Folder</button>' +
    "</div></section>";

  document.getElementById("select-existing").onclick = function () {
    chooseRoot("use_existing_data_root");
  };
  document.getElementById("select-new").onclick = function () {
    chooseRoot("create_data_root");
  };
}

function statusLabel(status) {
  return String(status || "PREPARING").replaceAll("_", " ").toUpperCase();
}

function projectCard(item, readOnly) {
  const project = item.project;
  const checked = gpuWorkbenchState.selectedProjectIds.has(project.id) ? " checked" : "";
  return (
    '<article class="project-card">' +
    '<label class="batch-select"><input class="project-batch-checkbox" type="checkbox" data-id="' +
    escapeHtml(project.id) +
    '"' +
    checked +
    ' /><span>GPU BATCH</span></label>' +
    '<div class="project-main">' +
    '<div class="project-title">' + escapeHtml(project.title) + "</div>" +
    '<div class="project-meta"><span class="status">' +
    escapeHtml(statusLabel(item.status)) +
    "</span>" +
    escapeHtml(project.id) +
    "</div></div>" +
    '<div class="project-actions">' +
    '<button class="icon-btn rename-project" data-id="' +
    escapeHtml(project.id) +
    '" data-title="' +
    escapeHtml(project.title) +
    '"' +
    (readOnly ? " disabled" : "") +
    ">Rename</button>" +
    '<button class="icon-btn delete-project" data-id="' +
    escapeHtml(project.id) +
    '" data-title="' +
    escapeHtml(project.title) +
    '"' +
    (readOnly ? " disabled" : "") +
    ">Delete</button>" +
    "</div></article>"
  );
}

function llmStateLabel(state) {
  switch (state) {
    case "ready":
      return "READY";
    case "needs_api_key":
      return "NEEDS API KEY";
    case "offline":
      return "OFFLINE";
    case "degraded":
      return "CHECK SETUP";
    default:
      return statusLabel(state);
  }
}

function llmModelOptions(status) {
  const models = Array.isArray(status.models) ? status.models : [];
  const ids = new Set();
  const rows = [];

  if (status.default_model) {
    ids.add(status.default_model);
    rows.push(
      '<option value="' +
        escapeHtml(status.default_model) +
        '">' +
        escapeHtml(status.default_model) +
        "</option>",
    );
  }

  models.forEach(function (model) {
    if (ids.has(model.id)) return;
    ids.add(model.id);
    rows.push(
      '<option value="' +
        escapeHtml(model.id) +
        '">' +
        escapeHtml(model.display_name || model.id) +
        (model.is_virtual ? " · virtual" : "") +
        "</option>",
    );
  });

  return rows.join("");
}

function renderLlmGatewayPanel(status) {
  const panel = document.getElementById("llmgateway-panel");
  if (!panel) return;

  const state = String(status.state || "offline");
  const health = status.health_status
    ? '<div class="info-row"><div class="info-label">HEALTH</div><div class="info-value">' +
      escapeHtml(status.health_status) +
      "</div></div>"
    : "";
  const discovered = Array.isArray(status.models) ? status.models.length : 0;

  panel.innerHTML =
    '<div class="panel-heading">' +
    '<div><p class="eyebrow">LLM ROUTER</p><h3>LLMGateway</h3></div>' +
    '<span class="llm-state ' +
    escapeHtml(state) +
    '">' +
    escapeHtml(llmStateLabel(state)) +
    "</span></div>" +
    '<p class="muted compact">' +
    escapeHtml(status.message) +
    "</p>" +
    '<div class="field compact-field"><label>ENDPOINT</label><input id="llmgateway-url" value="' +
    escapeHtml(status.base_url) +
    '" /></div>' +
    '<div class="field compact-field"><label>API KEY ENVIRONMENT VARIABLE</label><input id="llmgateway-key-env" value="' +
    escapeHtml(status.api_key_env) +
    '" /></div>' +
    '<div class="field compact-field"><label>MODEL POLICY</label><input id="llmgateway-model" list="llmgateway-model-options" value="' +
    escapeHtml(status.default_model) +
    '" /><datalist id="llmgateway-model-options">' +
    llmModelOptions(status) +
    "</datalist></div>" +
    '<div class="info-list llm-meta">' +
    health +
    '<div class="info-row"><div class="info-label">DISCOVERED MODELS</div><div class="info-value">' +
    escapeHtml(discovered) +
    " · virtual models first</div></div>" +
    '<div class="info-row"><div class="info-label">SECRET STORAGE</div><div class="info-value">Machine environment only · never Data Root</div></div>' +
    "</div>" +
    '<div class="actions compact-actions">' +
    '<button class="btn primary" id="llmgateway-save">Save &amp; Check</button>' +
    '<button class="btn" id="llmgateway-refresh">Refresh</button>' +
    "</div>";

  document.getElementById("llmgateway-save").onclick = async function () {
    const baseUrl = document.getElementById("llmgateway-url").value.trim();
    const apiKeyEnv = document.getElementById("llmgateway-key-env").value.trim();
    const defaultModel = document.getElementById("llmgateway-model").value.trim();

    const updated = await call("save_llmgateway_settings", {
      baseUrl: baseUrl,
      apiKeyEnv: apiKeyEnv,
      defaultModel: defaultModel,
    });
    renderLlmGatewayPanel(updated);
  };

  document.getElementById("llmgateway-refresh").onclick = async function () {
    renderLlmGatewayPanel(await call("llmgateway_status"));
  };
}

async function loadLlmGatewayPanel() {
  const panel = document.getElementById("llmgateway-panel");
  if (!panel) return;

  try {
    renderLlmGatewayPanel(await call("llmgateway_status"));
  } catch (_error) {
    panel.innerHTML =
      '<p class="eyebrow">LLM ROUTER</p><h3>LLMGateway</h3>' +
      '<div class="notice">Could not load LLMGateway settings. Check the desktop configuration and try again.</div>';
  }
}


function gpuWeekStartIso() {
  const now = new Date();
  const mondayOffset = (now.getDay() + 6) % 7;
  const start = new Date(now);
  start.setDate(now.getDate() - mondayOffset);
  start.setHours(0, 0, 0, 0);
  return start.toISOString();
}

function formatDuration(seconds) {
  const value = Number(seconds);
  if (!Number.isFinite(value)) return "unknown";
  if (value < 60) return Math.round(value) + " sec";
  if (value < 3600) return Math.round(value / 60) + " min";
  return (value / 3600).toFixed(value >= 36000 ? 0 : 1) + " h";
}

function runtimeContextJson() {
  return JSON.stringify(gpuWorkbenchState.context, null, 2);
}

function advancedGpuDetails(review) {
  const hashes = review
    ? '<div class="advanced-hashes"><div><span>BATCH SNAPSHOT</span><code>' +
      escapeHtml(review.batch.snapshot_hash) +
      '</code></div><div><span>SCHEDULE</span><code>' +
      escapeHtml(review.burst.schedule_hash) +
      "</code></div></div>"
    : "";
  const provider = gpuWorkbenchState.provider || {};
  return (
    '<details class="advanced-details"><summary>Advanced / provider runtime details</summary>' +
    '<p class="muted compact">Machine-local endpoint settings and provider-neutral preparation/spec snapshots live here. Store only environment-variable names, never secret values, in portable data.</p>' +
    hashes +
    '<div class="provider-config-grid">' +
    '<div class="field"><label>PROVIDER ID</label><input id="compute-provider-id" value="' +
    escapeHtml(provider.provider_id || "remote-gpu") +
    '" /></div>' +
    '<div class="field"><label>WORKER BASE URL</label><input id="compute-provider-url" value="' +
    escapeHtml(provider.base_url || "http://127.0.0.1:8787") +
    '" /></div>' +
    '<div class="field"><label>BEARER TOKEN ENV</label><input id="compute-provider-token-env" value="' +
    escapeHtml(provider.bearer_token_env || "OMNICREATOR_COMPUTE_TOKEN") +
    '" /></div></div>' +
    '<div class="actions compact-actions"><button class="btn" id="connect-gpu-provider-advanced">Connect with these settings</button></div>' +
    '<div class="field"><label>PREPARATION / EXECUTION CONTEXT JSON</label><textarea id="gpu-runtime-context" rows="12">' +
    escapeHtml(runtimeContextJson()) +
    "</textarea></div>" +
    '<div class="actions compact-actions"><button class="btn" id="apply-gpu-context">Apply context &amp; review again</button></div>' +
    "</details>"
  );
}

function providerReady() {
  return gpuWorkbenchState.provider && gpuWorkbenchState.provider.state === "READY";
}

function executionSpecsReady(review) {
  if (!review) return false;
  const expected = (review.batch.ready_jobs || [])
    .map(function (job) { return job.job_id; })
    .sort();
  const actual = (gpuWorkbenchState.context.execution_specs || [])
    .map(function (spec) { return spec.job_id; })
    .sort();
  return expected.length === actual.length &&
    expected.every(function (jobId, index) { return actual[index] === jobId; });
}

function providerCardMarkup(readOnly) {
  const provider = gpuWorkbenchState.provider || {
    state: "DISCONNECTED",
    provider_id: "remote-gpu",
    base_url: "http://127.0.0.1:8787",
    credential_present: false,
    message: "Compute worker status has not been loaded.",
  };
  const ready = provider.state === "READY";
  const action = ready
    ? '<button class="btn" id="disconnect-gpu-provider"' + (readOnly ? " disabled" : "") + '>Disconnect</button>'
    : '<button class="btn primary" id="connect-gpu-provider"' + (readOnly ? " disabled" : "") + '>Connect Provider</button>';
  return (
    '<div class="provider-card ' + (ready ? "connected" : "") + '">' +
    '<div><span class="provider-state">' + escapeHtml(provider.state) + '</span>' +
    '<strong>' + escapeHtml(provider.provider_id || "remote-gpu") + '</strong>' +
    '<p>' + escapeHtml(provider.message || "") + '</p></div>' +
    '<div class="provider-actions"><span>' + escapeHtml(provider.session_id || provider.base_url || "") + '</span>' +
    action + "</div></div>"
  );
}

async function loadComputeProviderStatus(readOnly) {
  try {
    gpuWorkbenchState.provider = await call("compute_provider_status");
  } catch (_error) {
    gpuWorkbenchState.provider = null;
  }
  renderGpuWorkbench(gpuWorkbenchState.review, readOnly);
}

async function connectGpuProvider(readOnly, fromAdvanced) {
  if (readOnly) return;
  const current = gpuWorkbenchState.provider || {};
  const providerIdInput = document.getElementById("compute-provider-id");
  const baseUrlInput = document.getElementById("compute-provider-url");
  const tokenInput = document.getElementById("compute-provider-token-env");
  const providerId = fromAdvanced && providerIdInput
    ? providerIdInput.value
    : current.provider_id || "remote-gpu";
  const baseUrl = fromAdvanced && baseUrlInput
    ? baseUrlInput.value
    : current.base_url || "http://127.0.0.1:8787";
  const bearerTokenEnv = fromAdvanced && tokenInput
    ? tokenInput.value
    : current.bearer_token_env || "OMNICREATOR_COMPUTE_TOKEN";

  gpuWorkbenchState.provider = await call("connect_compute_provider", {
    providerId: providerId,
    baseUrl: baseUrl,
    bearerTokenEnv: bearerTokenEnv || null,
  });
  showToast("Compute provider connected and capabilities discovered.");
  if (gpuWorkbenchState.selectedProjectIds.size) {
    await prepareGpuWorkbench(readOnly);
  } else {
    renderGpuWorkbench(gpuWorkbenchState.review, readOnly);
  }
}

function bindProviderControls(readOnly) {
  const connect = document.getElementById("connect-gpu-provider");
  if (connect) {
    connect.onclick = function () { connectGpuProvider(readOnly, false); };
  }
  const advancedConnect = document.getElementById("connect-gpu-provider-advanced");
  if (advancedConnect) {
    advancedConnect.onclick = function () { connectGpuProvider(readOnly, true); };
  }
  const disconnect = document.getElementById("disconnect-gpu-provider");
  if (disconnect) {
    disconnect.onclick = async function () {
      gpuWorkbenchState.provider = await call("disconnect_compute_provider");
      stopBurstSync();
      if (gpuWorkbenchState.selectedProjectIds.size) {
        await prepareGpuWorkbench(readOnly);
      } else {
        renderGpuWorkbench(gpuWorkbenchState.review, readOnly);
      }
    };
  }
}

function stopBurstSync() {
  if (gpuWorkbenchState.syncTimer) {
    clearInterval(gpuWorkbenchState.syncTimer);
    gpuWorkbenchState.syncTimer = null;
  }
}

async function syncBurstOnce(readOnly) {
  const projectIds = Array.from(gpuWorkbenchState.selectedProjectIds).sort();
  if (!projectIds.length || !providerReady()) {
    stopBurstSync();
    return;
  }
  try {
    const synced = await call("sync_compute_burst", { projectIds: projectIds });
    gpuWorkbenchState.provider = synced.provider;
    if (gpuWorkbenchState.review) {
      gpuWorkbenchState.review.queues = synced.queues;
      renderGpuWorkbench(gpuWorkbenchState.review, readOnly);
    }
    if (!(synced.queues.running || []).length) {
      stopBurstSync();
    }
  } catch (_error) {
    stopBurstSync();
  }
}

function startBurstSync(readOnly) {
  stopBurstSync();
  gpuWorkbenchState.syncTimer = setInterval(function () {
    syncBurstOnce(readOnly);
  }, 2500);
}

function blockedJobMarkup(job) {
  const reasons = Array.isArray(job.eligibility && job.eligibility.reasons)
    ? job.eligibility.reasons
    : [];
  const actions = reasons.length
    ? reasons
        .map(function (reason) {
          return (
            '<li><strong>' +
            escapeHtml(statusLabel(reason.code)) +
            "</strong><span>" +
            escapeHtml(reason.message) +
            "</span></li>"
          );
        })
        .join("")
    : "<li><span>Re-run preflight to refresh the canonical reason.</span></li>";
  return (
    '<article class="blocked-job"><div><strong>' +
    escapeHtml(job.step + " · " + job.unit) +
    '</strong><span class="mini-meta">' +
    escapeHtml(job.project_id) +
    "</span></div><ul>" +
    actions +
    "</ul></article>"
  );
}

function queueLane(title, items) {
  const rows = items.length
    ? items
        .map(function (entry) {
          const attempts = Array.isArray(entry.attempts) ? entry.attempts : [];
          const lastAttempt = attempts.length ? attempts[attempts.length - 1] : null;
          const error = lastAttempt && lastAttempt.error_code
            ? '<span class="queue-error">' + escapeHtml(lastAttempt.error_code) + "</span>"
            : "";
          return (
            '<div class="queue-job"><strong>' +
            escapeHtml(entry.job.step + " · " + entry.job.unit) +
            '</strong><span>' +
            escapeHtml(statusLabel(entry.job.status)) +
            " · " +
            escapeHtml(entry.job.project_id) +
            "</span>" +
            error +
            "</div>"
          );
        })
        .join("")
    : '<div class="queue-empty">None</div>';
  return (
    '<section class="queue-lane"><div class="queue-heading">' +
    escapeHtml(title) +
    '<span>' +
    escapeHtml(items.length) +
    "</span></div>" +
    rows +
    "</section>"
  );
}

function scheduleMarkup(review) {
  const waves = Array.isArray(review.burst.waves) ? review.burst.waves : [];
  if (!waves.length) {
    return '<div class="empty compact-empty">No runnable device waves yet.</div>';
  }
  return waves
    .map(function (wave) {
      const assignments = (wave.assignments || [])
        .map(function (assignment) {
          return (
            '<div class="wave-assignment"><strong>' +
            escapeHtml(assignment.affinity.model_group) +
            '</strong><span>' +
            escapeHtml(assignment.selection.device_id) +
            " · " +
            escapeHtml(assignment.step + "/" + assignment.unit) +
            "</span></div>"
          );
        })
        .join("");
      return (
        '<article class="wave"><div class="wave-title">Wave ' +
        escapeHtml(Number(wave.wave_index) + 1) +
        "</div>" +
        assignments +
        "</article>"
      );
    })
    .join("");
}

function renderGpuWorkbench(review, readOnly) {
  const panel = document.getElementById("gpu-workbench");
  if (!panel) return;

  if (!review) {
    panel.innerHTML =
      '<div class="workbench-empty"><p class="eyebrow">GPU WORKBENCH</p>' +
      '<h3>Prepare expensive work before opening GPU time.</h3>' +
      '<p class="muted">Select one or more projects, prepare the batch locally, then connect compute only when the reviewed queue is worth spending GPU time on.</p>' +
      providerCardMarkup(readOnly) +
      '<div class="notice subtle">No batch has been reviewed yet. Preparation/spec context stays provider-neutral; endpoint credentials remain machine-local through environment variables.</div>' +
      advancedGpuDetails(null) +
      "</div>";
    bindGpuWorkbenchAdvanced(readOnly);
    bindProviderControls(readOnly);
    return;
  }

  const batch = review.batch;
  const workload = review.workload;
  const queues = review.queues;
  const uncertainty = workload.unknown_jobs
    ? '<div class="uncertainty">Estimate is incomplete: <strong>' +
      escapeHtml(workload.unknown_jobs) +
      "</strong> ready job(s) have no trustworthy runtime history.</div>"
    : '<div class="confidence">All ready jobs have runtime history for their exact provider/device/model key.</div>';

  let workloadText = "No ready GPU work";
  if (workload.estimated_jobs && workload.unknown_jobs) {
    workloadText =
      formatDuration(workload.estimated_runtime_seconds) +
      " known + " +
      workload.unknown_jobs +
      " unknown job(s)";
  } else if (workload.estimated_jobs) {
    workloadText = formatDuration(workload.estimated_runtime_seconds) + " estimated serial work";
  } else if (workload.unknown_jobs) {
    workloadText = workload.unknown_jobs + " job(s), runtime unknown";
  }

  let budgetMarkup =
    '<div class="metric-value">Not configured</div><div class="metric-note">Set an allowance after one provider is selected.</div>';
  if (review.budget) {
    const budget = review.budget.weekly_budget;
    budgetMarkup =
      '<div class="metric-value">' +
      escapeHtml(formatDuration(budget.remaining_session_seconds)) +
      ' remaining</div><div class="metric-note">' +
      escapeHtml(formatDuration(budget.used_session_seconds)) +
      " used of " +
      escapeHtml(formatDuration(budget.allowance_seconds)) +
      " · " +
      escapeHtml(statusLabel(review.budget.serial_budget_signal)) +
      "</div>";
  }

  const readyProvider = batch.ready_jobs.length
    ? batch.ready_jobs[0].eligibility.selection &&
      batch.ready_jobs[0].eligibility.selection.provider_id
    : "";
  const budgetAction = readyProvider
    ? '<div class="budget-set"><input id="gpu-budget-hours" type="number" min="0.1" step="0.5" placeholder="30" /><button class="btn" id="set-gpu-budget">Set weekly hours</button></div>'
    : "";

  const blocked = batch.blocked_jobs.length
    ? batch.blocked_jobs.map(blockedJobMarkup).join("")
    : '<div class="confidence">No blocked jobs in the reviewed batch.</div>';
  const specsReady = executionSpecsReady(review);
  const canStart = review.startable && providerReady() && specsReady && !readOnly;
  const launchNotice = !providerReady()
    ? '<div class="uncertainty">Connect the reviewed compute provider before Burst Mode.</div>'
    : !specsReady
      ? '<div class="uncertainty">Execution specs are incomplete. Ready jobs require one immutable provider-neutral operation/payload spec each.</div>'
      : "";

  panel.innerHTML =
    '<div class="workbench-head"><div><p class="eyebrow">GPU WORKBENCH</p><h3>Reviewed batch</h3></div>' +
    '<span class="review-state ' +
    (review.startable ? "ready" : "blocked") +
    '">' +
    (review.startable ? "READY TO BURST" : "ACTION NEEDED") +
    "</span></div>" +
    '<div class="metric-grid">' +
    '<div class="metric"><span>CANDIDATES</span><div class="metric-value">' +
    escapeHtml(batch.candidate_jobs) +
    '</div><div class="metric-note">' +
    escapeHtml(batch.ready_jobs.length) +
    " ready · " +
    escapeHtml(batch.blocked_jobs.length) +
    " blocked</div></div>" +
    '<div class="metric"><span>WORKLOAD</span><div class="metric-value">' +
    escapeHtml(workloadText) +
    '</div><div class="metric-note">History-based EMA; unknown work is never guessed.</div></div>' +
    '<div class="metric"><span>WEEKLY GPU BUDGET</span>' +
    budgetMarkup +
    budgetAction +
    "</div>" +
    '<div class="metric"><span>SCHEDULE</span><div class="metric-value">' +
    escapeHtml(review.burst.waves.length) +
    ' wave(s)</div><div class="metric-note">' +
    escapeHtml(review.burst.devices.length) +
    " independent device(s), no VRAM pooling</div></div></div>" +
    uncertainty +
    providerCardMarkup(readOnly) +
    launchNotice +
    '<div class="workbench-section"><div class="section-heading"><h4>Preflight</h4><span>Actionable blockers</span></div>' +
    blocked +
    "</div>" +
    '<div class="workbench-section"><div class="section-heading"><h4>Model-group / device waves</h4><span>Deterministic preview</span></div><div class="waves">' +
    scheduleMarkup(review) +
    "</div></div>" +
    '<div class="workbench-section"><div class="section-heading"><h4>Canonical queues</h4><span>Derived from jobs + attempts</span></div><div class="queue-grid">' +
    queueLane("RUNNING", queues.running || []) +
    queueLane("COMPLETED", queues.completed || []) +
    queueLane("REMAINING", queues.remaining || []) +
    queueLane("RETRYABLE", queues.retryable || []) +
    "</div></div>" +
    '<div class="burst-bar"><div><strong>Burst Mode</strong><span>Non-interactive execution policy · error-aware retry · immediate verified local artifact commit.</span></div>' +
    '<button class="btn primary" id="start-gpu-burst"' +
    (canStart ? "" : " disabled") +
    ">Start Burst Mode</button></div>" +
    advancedGpuDetails(review);

  bindGpuWorkbenchAdvanced(readOnly);
  bindProviderControls(readOnly);

  const budgetButton = document.getElementById("set-gpu-budget");
  if (budgetButton) {
    budgetButton.onclick = async function () {
      const hours = Number(document.getElementById("gpu-budget-hours").value);
      if (!Number.isFinite(hours) || hours <= 0) {
        showToast("Enter a positive weekly GPU allowance in hours.");
        return;
      }
      await call("set_gpu_weekly_budget", {
        providerId: readyProvider,
        allowanceHours: hours,
      });
      await prepareGpuWorkbench(readOnly);
    };
  }

  const startButton = document.getElementById("start-gpu-burst");
  if (startButton) {
    startButton.onclick = async function () {
      const started = await call("start_gpu_burst", {
        input: {
          reviewed_batch: review.batch,
          providers: [],
          execution_specs: gpuWorkbenchState.context.execution_specs,
          expected_schedule_hash: review.burst.schedule_hash,
        },
      });
      const dispatched = started.dispatch.dispatched.length;
      const failures = started.dispatch.failures.length;
      showToast(
        "Burst Mode dispatched " + dispatched + " job(s)" +
        (failures ? "; " + failures + " entered canonical retry handling." : "."),
      );
      gpuWorkbenchState.review.queues = started.queues;
      gpuWorkbenchState.review.burst = started.dispatch.burst;
      renderGpuWorkbench(gpuWorkbenchState.review, readOnly);
      if ((started.queues.running || []).length) startBurstSync(readOnly);
    };
  }
}

function bindGpuWorkbenchAdvanced(readOnly) {
  const button = document.getElementById("apply-gpu-context");
  if (!button) return;
  button.disabled = false;
  button.onclick = async function () {
    const editor = document.getElementById("gpu-runtime-context");
    try {
      const parsed = JSON.parse(editor.value || "{}");
      gpuWorkbenchState.context = {
        preparations: Array.isArray(parsed.preparations) ? parsed.preparations : [],
        providers: Array.isArray(parsed.providers) ? parsed.providers : [],
        running: Array.isArray(parsed.running) ? parsed.running : [],
        execution_specs: Array.isArray(parsed.execution_specs) ? parsed.execution_specs : [],
      };
    } catch (_error) {
      showToast("Runtime context must be valid JSON.");
      return;
    }
    await prepareGpuWorkbench(readOnly);
  };
}

async function prepareGpuWorkbench(readOnly) {
  const projectIds = Array.from(gpuWorkbenchState.selectedProjectIds).sort();
  if (!projectIds.length) {
    showToast("Select at least one project for the GPU batch.");
    return;
  }
  const review = await call("gpu_workbench_review", {
    input: {
      project_ids: projectIds,
      preparations: gpuWorkbenchState.context.preparations,
      providers: gpuWorkbenchState.context.providers,
      running: gpuWorkbenchState.context.running,
      week_start: gpuWeekStartIso(),
    },
  });
  gpuWorkbenchState.review = review;
  renderGpuWorkbench(review, readOnly);
}

function renderWorkspace(snapshot) {
  const workspace = snapshot.workspace;
  const projects = snapshot.projects;
  setMode(workspace.read_only ? "READ ONLY" : "WRITER", workspace.read_only ? "read-only" : "writer");

  const projectIds = new Set(projects.map(function (item) { return item.project.id; }));
  Array.from(gpuWorkbenchState.selectedProjectIds).forEach(function (projectId) {
    if (!projectIds.has(projectId)) gpuWorkbenchState.selectedProjectIds.delete(projectId);
  });

  const cards = projects.length
    ? projects.map(function (item) { return projectCard(item, workspace.read_only); }).join("")
    : '<div class="empty">No projects yet. Create the first production to verify portable state.</div>';

  const readOnlyNotice = workspace.read_only
    ? '<div class="notice">Read-only mode. Project state is visible, but no production data will be changed.</div>'
    : "";

  content.innerHTML =
    '<div class="workspace-grid">' +
    '<section class="panel">' +
    '<p class="eyebrow">PROJECT BOARD</p><h2>Productions</h2>' +
    readOnlyNotice +
    '<div class="toolbar">' +
    '<div class="field"><label>NEW PROJECT</label><input id="new-project-title" placeholder="When God Seems Silent"' +
    (workspace.read_only ? " disabled" : "") +
    " /></div>" +
    '<button class="btn primary" id="create-project"' +
    (workspace.read_only ? " disabled" : "") +
    ">Create</button></div>" +
    '<div class="project-list">' + cards + "</div>" +
    '<div class="batch-toolbar"><div><strong id="gpu-selected-count">' +
    escapeHtml(gpuWorkbenchState.selectedProjectIds.size) +
    ' selected</strong><span>Prepare projects before consuming GPU quota.</span></div>' +
    '<button class="btn primary" id="prepare-gpu-batch">Prepare GPU Batch</button></div>' +
    '<div id="gpu-workbench" class="gpu-workbench"></div></section>' +
    '<div class="sidebar-stack"><aside class="panel">' +
    '<p class="eyebrow">DATA & PORTABILITY</p><h3>Workspace</h3>' +
    '<div class="info-list">' +
    '<div class="info-row"><div class="info-label">DATA FOLDER</div><div class="info-value">' + escapeHtml(workspace.data_root) + "</div></div>" +
    '<div class="info-row"><div class="info-label">WORKSPACE ID</div><div class="info-value hash">' + escapeHtml(workspace.workspace_id) + "</div></div>" +
    '<div class="info-row"><div class="info-label">REVISION</div><div class="info-value">' + escapeHtml(workspace.revision) + "</div></div>" +
    '<div class="info-row"><div class="info-label">SESSION</div><div class="info-value">' +
    (workspace.read_only ? "Read only" : "Single writer active") +
    "</div></div></div>" +
    '<div class="actions">' +
    '<button class="btn primary" id="handoff"' + (workspace.read_only ? " disabled" : "") + ">Prepare for Device Handoff</button>" +
    '<button class="btn" id="change-root">Change Data Folder</button>' +
    '</div></aside><aside class="panel" id="llmgateway-panel">' +
    '<p class="eyebrow">LLM ROUTER</p><h3>LLMGateway</h3>' +
    '<p class="muted">Checking local gateway connection…</p>' +
    "</aside></div></div>";

  const createButton = document.getElementById("create-project");
  if (createButton) {
    createButton.onclick = async function () {
      const input = document.getElementById("new-project-title");
      const title = input.value.trim();
      if (!title) {
        showToast("Enter a project title first.");
        return;
      }
      render(await call("create_project", { title: title }));
    };
  }

  document.querySelectorAll(".project-batch-checkbox").forEach(function (checkbox) {
    checkbox.onchange = function () {
      if (checkbox.checked) {
        gpuWorkbenchState.selectedProjectIds.add(checkbox.dataset.id);
      } else {
        gpuWorkbenchState.selectedProjectIds.delete(checkbox.dataset.id);
      }
      const count = document.getElementById("gpu-selected-count");
      if (count) count.textContent = gpuWorkbenchState.selectedProjectIds.size + " selected";
      gpuWorkbenchState.review = null;
      renderGpuWorkbench(null, workspace.read_only);
    };
  });

  document.getElementById("prepare-gpu-batch").onclick = function () {
    prepareGpuWorkbench(workspace.read_only);
  };

  document.querySelectorAll(".rename-project").forEach(function (button) {
    button.onclick = async function () {
      const title = window.prompt("Rename project", button.dataset.title);
      if (!title || !title.trim()) return;
      render(await call("rename_project", {
        projectId: button.dataset.id,
        title: title.trim(),
      }));
    };
  });

  document.querySelectorAll(".delete-project").forEach(function (button) {
    button.onclick = async function () {
      if (!window.confirm('Delete "' + button.dataset.title + '"?')) return;
      render(await call("delete_project", { projectId: button.dataset.id }));
    };
  });

  document.getElementById("handoff").onclick = async function () {
    render(await call("prepare_device_handoff"));
  };
  document.getElementById("change-root").onclick = renderFirstLaunch;
  renderGpuWorkbench(gpuWorkbenchState.review, workspace.read_only);
  loadComputeProviderStatus(workspace.read_only);
  loadLlmGatewayPanel();
}

function renderHandoff(snapshot) {
  setMode("SAFE TO SWITCH", "writer");
  content.innerHTML =
    '<section class="hero-card">' +
    '<p class="eyebrow">DEVICE HANDOFF READY</p>' +
    '<h1>Workspace snapshot is clean and verified.</h1>' +
    '<p class="muted">Close OmniCreator, allow your sync provider to finish, then open the same Data Folder on the other machine.</p>' +
    '<div class="info-list">' +
    '<div class="info-row"><div class="info-label">DATA FOLDER</div><div class="info-value">' + escapeHtml(snapshot.data_root) + "</div></div>" +
    '<div class="info-row"><div class="info-label">REVISION</div><div class="info-value">' + escapeHtml(snapshot.revision) + "</div></div>" +
    '<div class="info-row"><div class="info-label">SNAPSHOT SHA-256</div><div class="info-value hash">' + escapeHtml(snapshot.snapshot_sha256) + "</div></div>" +
    "</div>" +
    '<div class="actions"><button class="btn" id="reopen">Continue on This Machine</button></div>' +
    "</section>";

  document.getElementById("reopen").onclick = async function () {
    render(await call("bootstrap"));
  };
}

async function start() {
  try {
    render(await call("bootstrap"));
  } catch (_error) {
    setMode("ERROR", "read-only");
  }
}

setInterval(function () {
  invoke("heartbeat").catch(function () {});
}, 30000);

start();
