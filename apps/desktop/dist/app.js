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
const productionPackState = {
  selectedProjectId: null,
  draftTextByProject: new Map(),
  viewByProject: new Map(),
};
const studioPackState = {
  catalog: null,
  review: null,
  selectedPackId: null,
  mode: "basic",
  editingProjectId: null,
  editingPackItemId: null,
  draftTitle: "",
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
    '<div class="project-action-summary">' +
    escapeHtml(item.board && item.board.summary ? item.board.summary : "Review project state.") +
    "</div>" +
    '<div class="project-meta"><span class="status">' +
    escapeHtml(statusLabel(item.status)) +
    "</span>" +
    (project.studio_pack
      ? '<span class="studio-pack-project-tag">' + escapeHtml(project.studio_pack) + "</span>"
      : '<span class="studio-pack-project-tag missing">NO STUDIO PACK</span>') +
    "</div></div>" +
    '<div class="project-actions">' +
    (item.board && item.board.column === "NEEDS_REVIEW"
      ? '<button class="icon-btn project-review-center" data-id="' +
        escapeHtml(project.id) +
        '">Review Issues</button>'
      : "") +
    '<button class="icon-btn studio-pack-project" data-id="' +
    escapeHtml(project.id) +
    '">Studio Pack</button>' +
    '<button class="icon-btn production-pack-project" data-id="' +
    escapeHtml(project.id) +
    '">Export to Resolve</button>' +
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

const projectBoardColumns = [
  { id: "IDEAS", label: "Ideas" },
  { id: "PREPARING", label: "Preparing" },
  { id: "NEEDS_REVIEW", label: "Needs Review" },
  { id: "GPU_READY", label: "GPU Ready" },
  { id: "GPU_RUNNING", label: "GPU Running" },
  { id: "READY_TO_EDIT", label: "Ready to Edit" },
  { id: "DONE", label: "Done" },
];

function renderProjectKanban(projects, readOnly) {
  return (
    '<div class="project-kanban" aria-label="Project Kanban">' +
    projectBoardColumns
      .map(function (column) {
        const items = projects.filter(function (item) {
          return item.board && item.board.column === column.id;
        });
        const cards = items.length
          ? items
              .map(function (item) {
                return projectCard(item, readOnly);
              })
              .join("")
          : '<div class="kanban-empty">No projects</div>';
        return (
          '<section class="kanban-column" data-board-column="' +
          escapeHtml(column.id) +
          '"><div class="kanban-column-head"><strong>' +
          escapeHtml(column.label) +
          '</strong><span>' +
          escapeHtml(items.length) +
          "</span></div><div class=\"kanban-column-body\">" +
          cards +
          "</div></section>"
        );
      })
      .join("") +
    "</div>"
  );
}

function defaultProductionPack(project) {
  return {
    schema: "omnicreator.production-pack",
    version: 1,
    project_id: project.id,
    title: project.title,
    frame_rate: { numerator: 24, denominator: 1 },
    tracks: [],
    subtitles: [],
    markers: [],
  };
}

function productionPackDraftText(project, view) {
  if (productionPackState.draftTextByProject.has(project.id)) {
    return productionPackState.draftTextByProject.get(project.id);
  }
  const value = view && view.last_pack ? view.last_pack : defaultProductionPack(project);
  const text = JSON.stringify(value, null, 2);
  productionPackState.draftTextByProject.set(project.id, text);
  return text;
}

function productionHistoryMarkup(view) {
  const history = view && Array.isArray(view.history) ? view.history : [];
  if (!history.length) {
    return '<div class="queue-empty">No canonical export Job yet.</div>';
  }
  return history
    .slice(0, 4)
    .map(function (entry) {
      const attempts = Array.isArray(entry.attempts) ? entry.attempts : [];
      const lastAttempt = attempts.length ? attempts[attempts.length - 1] : null;
      const error = lastAttempt && lastAttempt.error_code
        ? " · " + lastAttempt.error_code
        : "";
      return (
        '<div class="production-history-row"><strong>' +
        escapeHtml(statusLabel(entry.job.status)) +
        '</strong><span>' +
        escapeHtml(attempts.length + " attempt(s)" + error) +
        '</span><code>' +
        escapeHtml(entry.package_base_uri) +
        "</code></div>"
      );
    })
    .join("");
}

function productionDiagnosticMarkup(view) {
  const diagnostic = view && view.diagnostic;
  if (!diagnostic) return "";
  const identity = diagnostic.artifact_id
    ? '<div class="production-diagnostic-id"><strong>ARTIFACT</strong><code>' +
      escapeHtml(diagnostic.artifact_id) +
      "</code></div>"
    : "";
  const logical = diagnostic.logical_uri
    ? '<div class="production-diagnostic-id"><strong>LOGICAL URI</strong><code>' +
      escapeHtml(diagnostic.logical_uri) +
      "</code></div>"
    : "";
  return (
    '<div class="production-diagnostic ' +
    escapeHtml(diagnostic.kind || "export_failure") +
    '"><strong>' +
    escapeHtml(statusLabel(diagnostic.kind || "export_failure")) +
    "</strong><p>" +
    escapeHtml(diagnostic.message) +
    "</p>" +
    identity +
    logical +
    '<p class="diagnostic-action">' +
    escapeHtml(diagnostic.action) +
    "</p></div>"
  );
}

function renderProductionPackPanel(project, readOnly, view) {
  const panel = document.getElementById("production-pack-panel");
  if (!panel) return;

  if (!project) {
    panel.innerHTML =
      '<div class="production-pack-empty"><p class="eyebrow">DAVINCI PRODUCTION PACK</p>' +
      '<h3>Export an editable handoff without turning OmniCreator into an editor.</h3>' +
      '<p class="muted compact">Choose “Export to Resolve” on a project. Export state is derived from canonical Job / Attempt / Artifact history.</p></div>';
    return;
  }

  const history = view && Array.isArray(view.history) ? view.history : [];
  const latest = history.length ? history[0] : null;
  const state = view ? view.state : "not_exported";
  const packageUri = latest ? latest.package_base_uri : "";
  const cacheNote =
    view && view.outcome && view.outcome.cache_hit
      ? '<span class="production-cache-hit">VERIFIED CACHE HIT</span>'
      : "";
  const draftText = productionPackDraftText(project, view);
  const actionLabel = latest ? "Regenerate Production Pack" : "Export Production Pack";

  panel.innerHTML =
    '<div class="production-pack-head"><div><p class="eyebrow">DAVINCI PRODUCTION PACK</p><h3>' +
    escapeHtml(project.title) +
    '</h3></div><span class="review-state ' +
    (state === "succeeded" || state === "cached" ? "ready" : "blocked") +
    '">' +
    escapeHtml(statusLabel(state)) +
    "</span></div>" +
    '<div class="production-pack-body">' +
    (packageUri
      ? '<div class="info-row"><div class="info-label">LOGICAL PACKAGE LOCATION</div><div class="info-value hash">' +
        escapeHtml(packageUri) +
        "</div></div>"
      : '<div class="notice subtle">No committed production package yet. A successful export will expose a portable logical package location here.</div>') +
    cacheNote +
    productionDiagnosticMarkup(view) +
    '<div class="field production-pack-editor"><label>CANONICAL PRODUCTIONPACK V1 JSON</label><textarea id="production-pack-json" rows="13"' +
    (readOnly ? " readonly" : "") +
    ">" +
    escapeHtml(draftText) +
    "</textarea></div>" +
    '<div class="production-pack-actions"><button class="btn primary" id="run-production-export"' +
    (readOnly ? " disabled" : "") +
    ">" +
    actionLabel +
    '</button><button class="btn" id="refresh-production-export">Refresh Status</button></div>' +
    '<details class="advanced-details production-history"><summary>Canonical export history</summary>' +
    productionHistoryMarkup(view) +
    "</details></div>";

  const editor = document.getElementById("production-pack-json");
  if (editor) {
    editor.oninput = function () {
      productionPackState.draftTextByProject.set(project.id, editor.value);
    };
  }

  document.getElementById("refresh-production-export").onclick = function () {
    openProductionPack(project, readOnly);
  };

  const exportButton = document.getElementById("run-production-export");
  if (exportButton) {
    exportButton.onclick = async function () {
      let parsed;
      try {
        parsed = JSON.parse(editor.value);
      } catch (_error) {
        showToast("ProductionPack must be valid JSON.");
        return;
      }
      if (parsed.project_id !== project.id) {
        showToast("ProductionPack project_id must match the selected project.");
        return;
      }

      exportButton.disabled = true;
      try {
        const updated = await call("export_production_pack", {
          productionPack: parsed,
        });
        productionPackState.viewByProject.set(project.id, updated);
        if (!updated.diagnostic && updated.last_pack) {
          productionPackState.draftTextByProject.set(
            project.id,
            JSON.stringify(updated.last_pack, null, 2),
          );
        }
        renderProductionPackPanel(project, readOnly, updated);
        showToast(
          updated.state === "cached"
            ? "Production Pack verified from cache."
            : updated.diagnostic
              ? "Production Pack needs attention before export can succeed."
              : "Production Pack exported and committed.",
        );
      } finally {
        const current = document.getElementById("run-production-export");
        if (current && !readOnly) current.disabled = false;
      }
    };
  }
}

async function openProductionPack(project, readOnly) {
  productionPackState.selectedProjectId = project.id;
  renderProductionPackPanel(
    project,
    readOnly,
    productionPackState.viewByProject.get(project.id) || null,
  );
  const view = await call("production_export_status", { projectId: project.id });
  productionPackState.viewByProject.set(project.id, view);
  if (
    view.last_pack &&
    !productionPackState.draftTextByProject.has(project.id)
  ) {
    productionPackState.draftTextByProject.set(
      project.id,
      JSON.stringify(view.last_pack, null, 2),
    );
  }
  renderProductionPackPanel(project, readOnly, view);
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



function studioPackItems() {
  return studioPackState.catalog && Array.isArray(studioPackState.catalog.packs)
    ? studioPackState.catalog.packs
    : [];
}

function builtInStudioPackItems() {
  return studioPackItems().filter(function (item) {
    return !item.custom;
  });
}

function studioPackItem(id) {
  return studioPackItems().find(function (item) {
    return item.pack.id === id;
  }) || null;
}

function studioPackBaseId(item) {
  if (!item) return null;
  const lineage = Array.isArray(item.pack.lineage) ? item.pack.lineage : [];
  if (item.custom && lineage.length >= 2) {
    return lineage[lineage.length - 2];
  }
  return item.pack.id;
}

function studioPackAvailabilityClass(status) {
  switch (String(status || "")) {
    case "AVAILABLE":
      return "ready";
    case "AVAILABLE_WITH_SETUP":
      return "setup";
    default:
      return "blocked";
  }
}

function studioPackReasonText(reason) {
  const route = reason.route ? reason.route + ": " : "";
  const capability = reason.capability
    ? "requires " + reason.capability
    : statusLabel(reason.code);
  const plugin = reason.plugin_id ? " · " + reason.plugin_id : "";
  const runtime = reason.runtime_reason ? " · " + reason.runtime_reason : "";
  return route + capability + plugin + runtime;
}

function studioPackAvailabilityMarkup(pack) {
  const availability = pack.availability || { status: "UNAVAILABLE", reasons: [] };
  const reasons = Array.isArray(availability.reasons) ? availability.reasons : [];
  const blocking = reasons.filter(function (reason) {
    return reason.blocking;
  });
  const notes = blocking.length ? blocking : reasons.slice(0, 2);
  const detail = notes.length
    ? '<ul class="studio-pack-reasons">' +
      notes
        .map(function (reason) {
          return "<li>" + escapeHtml(studioPackReasonText(reason)) + "</li>";
        })
        .join("") +
      "</ul>"
    : '<p class="muted compact">All required capabilities are available.</p>';
  return (
    '<div class="studio-pack-availability ' +
    studioPackAvailabilityClass(availability.status) +
    '"><strong>' +
    escapeHtml(statusLabel(availability.status)) +
    "</strong>" +
    detail +
    "</div>"
  );
}

function studioPackPickerMarkup(selectedId) {
  const packs = builtInStudioPackItems();
  if (!packs.length) {
    return '<div class="empty compact-empty">No Studio Pack definitions are available.</div>';
  }
  return (
    '<div class="studio-pack-grid">' +
    packs
      .map(function (item) {
        const pack = item.pack;
        const selected = pack.id === selectedId;
        const unavailable =
          !pack.availability || pack.availability.status !== "AVAILABLE";
        return (
          '<button type="button" class="studio-pack-card ' +
          (selected ? "selected " : "") +
          studioPackAvailabilityClass(pack.availability && pack.availability.status) +
          '" data-pack-id="' +
          escapeHtml(pack.id) +
          '">' +
          '<div class="studio-pack-card-head"><strong>' +
          escapeHtml(pack.name) +
          '</strong><span>' +
          escapeHtml(statusLabel(pack.availability && pack.availability.status)) +
          "</span></div>" +
          '<p class="muted compact">' +
          escapeHtml(
            pack.automation && pack.automation.value
              ? statusLabel(pack.automation.value) + " automation"
              : "Studio Pack",
          ) +
          (unavailable ? " · setup required before creation" : "") +
          "</p>" +
          "</button>"
        );
      })
      .join("") +
    "</div>"
  );
}

function presetChoices(key) {
  const values = new Set();
  builtInStudioPackItems().forEach(function (item) {
    (item.pack.presets || []).forEach(function (preset) {
      if (preset.key === key) values.add(preset.value);
    });
  });
  return Array.from(values).sort();
}

function studioPackSourceItem(baseItem) {
  if (
    studioPackState.editingPackItemId &&
    studioPackBaseId(studioPackItem(studioPackState.editingPackItemId)) ===
      baseItem.pack.id
  ) {
    return studioPackItem(studioPackState.editingPackItemId) || baseItem;
  }
  return baseItem;
}

function studioPackCustomizeMarkup(baseItem) {
  const sourceItem = studioPackSourceItem(baseItem);
  const sourcePack = sourceItem.pack;
  const isCustom = Boolean(sourceItem.custom);
  const presetRows = (sourcePack.presets || [])
    .map(function (preset) {
      const explicit = isCustom && preset.source === "EXPLICIT_OVERRIDE";
      const options = presetChoices(preset.key)
        .map(function (value) {
          return (
            '<option value="' +
            escapeHtml(value) +
            '"' +
            (explicit && value === preset.value ? " selected" : "") +
            ">" +
            escapeHtml(statusLabel(value)) +
            "</option>"
          );
        })
        .join("");
      return (
        '<div class="field compact-field"><label>' +
        escapeHtml(statusLabel(preset.key)) +
        '</label><select class="studio-preset-override" data-preset-key="' +
        escapeHtml(preset.key) +
        '"><option value="">' +
        escapeHtml("Inherit · " + statusLabel(basePresetValue(baseItem, preset.key))) +
        "</option>" +
        options +
        "</select></div>"
      );
    })
    .join("");

  const automationExplicit =
    isCustom &&
    sourcePack.automation &&
    sourcePack.automation.source === "EXPLICIT_OVERRIDE";
  const automationValue = sourcePack.automation
    ? sourcePack.automation.value
    : "BALANCED";
  return (
    '<div class="studio-customize-grid">' +
    '<div class="field compact-field"><label>AUTOMATION LEVEL</label>' +
    '<select id="studio-automation-override">' +
    '<option value="">Inherit · ' +
    escapeHtml(statusLabel(baseItem.pack.automation.value)) +
    "</option>" +
    ["ASSISTED", "BALANCED", "AUTOPILOT"]
      .map(function (value) {
        return (
          '<option value="' +
          value +
          '"' +
          (automationExplicit && automationValue === value ? " selected" : "") +
          ">" +
          statusLabel(value) +
          "</option>"
        );
      })
      .join("") +
    "</select></div>" +
    presetRows +
    "</div>" +
    '<p class="muted compact">Inherited values remain linked to the selected Studio Pack. Only explicit creator overrides are stored in the portable child pack.</p>'
  );
}

function basePresetValue(baseItem, key) {
  const preset = (baseItem.pack.presets || []).find(function (candidate) {
    return candidate.key === key;
  });
  return preset ? preset.value : "default";
}

function baseQualityValue(baseItem, key) {
  const quality = (baseItem.pack.quality_thresholds || []).find(function (candidate) {
    return candidate.key === key;
  });
  return quality ? Number(quality.value) : 0;
}

function studioPackAdvancedMarkup(baseItem) {
  const sourceItem = studioPackSourceItem(baseItem);
  const routeRows = (sourceItem.pack.routes || [])
    .map(function (route) {
      const targets = (route.targets || [])
        .map(function (target, index) {
          return (
            '<li><span>' +
            escapeHtml(index === 0 ? "Preferred" : "Fallback " + index) +
            "</span><code>" +
            escapeHtml(target.plugin_type + " / " + target.capability) +
            (target.plugin_id ? " · " + escapeHtml(target.plugin_id) : "") +
            (target.preset ? " · " + escapeHtml(target.preset) : "") +
            "</code></li>"
          );
        })
        .join("");
      const unavailable = (route.availability_reasons || [])
        .filter(function (reason) {
          return reason.blocking;
        })
        .map(function (reason) {
          return "<small>" + escapeHtml(studioPackReasonText(reason)) + "</small>";
        })
        .join("");
      return (
        '<article class="studio-route-row"><strong>' +
        escapeHtml(route.key) +
        '</strong><span class="value-source">' +
        escapeHtml(statusLabel(route.source)) +
        "</span><ol>" +
        targets +
        "</ol>" +
        unavailable +
        "</article>"
      );
    })
    .join("");

  const qualityRows = (sourceItem.pack.quality_thresholds || [])
    .map(function (quality) {
      const baseValue = baseQualityValue(baseItem, quality.key);
      return (
        '<div class="field compact-field"><label>' +
        escapeHtml(statusLabel(quality.key) + " QUALITY") +
        '</label><input class="studio-quality-override" data-quality-key="' +
        escapeHtml(quality.key) +
        '" data-base-value="' +
        escapeHtml(baseValue) +
        '" type="number" min="0" max="100" value="' +
        escapeHtml(quality.value) +
        '" /></div>'
      );
    })
    .join("");

  return (
    '<div class="studio-advanced-note notice subtle">Advanced is a projection over canonical routes and existing machine-local controls. Provider endpoints, credentials and absolute paths are never written into the portable Studio Pack.</div>' +
    '<div class="studio-quality-grid">' +
    qualityRows +
    "</div>" +
    '<details class="advanced-details" open><summary>Resolved plugin / capability routing</summary>' +
    '<div class="studio-route-list">' +
    routeRows +
    "</div></details>" +
    '<p class="muted compact">LLMGateway routing and Compute Provider runtime controls remain in their existing panels on this screen. Production Pack export settings remain on the canonical DaVinci panel.</p>'
  );
}

function collectStudioPackOverrides(baseItem) {
  const overrides = {
    automation_level: null,
    routes: {},
    presets: {},
    quality_thresholds: {},
    remove_routes: [],
    remove_presets: [],
    remove_quality_thresholds: [],
  };
  const automation = document.getElementById("studio-automation-override");
  if (automation && automation.value) {
    overrides.automation_level = automation.value;
  }
  document.querySelectorAll(".studio-preset-override").forEach(function (input) {
    if (input.value) overrides.presets[input.dataset.presetKey] = input.value;
  });
  document.querySelectorAll(".studio-quality-override").forEach(function (input) {
    const value = Number(input.value);
    const base = Number(input.dataset.baseValue);
    if (Number.isFinite(value) && value !== base) {
      overrides.quality_thresholds[input.dataset.qualityKey] = value;
    }
  });
  return overrides;
}

function renderStudioPackCreator(projects, readOnly) {
  const panel = document.getElementById("studio-pack-creator");
  if (!panel) return;
  if (!studioPackState.catalog) {
    panel.innerHTML =
      '<p class="eyebrow">STUDIO PACK</p><h3>Loading creator workflow…</h3>';
    return;
  }

  const builtIns = builtInStudioPackItems();
  if (!studioPackState.selectedPackId) {
    const firstReady = builtIns.find(function (item) {
      return item.pack.availability.status === "AVAILABLE";
    });
    studioPackState.selectedPackId = firstReady
      ? firstReady.pack.id
      : builtIns.length
        ? builtIns[0].pack.id
        : null;
  }

  const editingProject = studioPackState.editingProjectId
    ? projects.find(function (item) {
        return item.project.id === studioPackState.editingProjectId;
      })
    : null;
  let baseItem = studioPackItem(studioPackState.selectedPackId);
  if (baseItem && baseItem.custom) {
    baseItem = studioPackItem(studioPackBaseId(baseItem));
  }
  if (!baseItem && builtIns.length) baseItem = builtIns[0];

  const selectedId = baseItem ? baseItem.pack.id : "";
  const mode = studioPackState.mode;
  const detail =
    mode === "customize" && baseItem
      ? studioPackCustomizeMarkup(baseItem)
      : mode === "advanced" && baseItem
        ? studioPackCustomizeMarkup(baseItem) + studioPackAdvancedMarkup(baseItem)
        : baseItem
          ? studioPackAvailabilityMarkup(baseItem.pack)
          : "";

  const header = editingProject
    ? '<div><p class="eyebrow">STUDIO PACK · PROJECT SETTINGS</p><h3>' +
      escapeHtml(editingProject.project.title) +
      '</h3><p class="muted compact">Change creative intent without creating parallel workflow state.</p></div>'
    : '<div><p class="eyebrow">NEW PRODUCTION · BASIC</p><h3>Start with a Studio Pack</h3><p class="muted compact">Choose the production style first. Plugin wiring stays out of the Basic flow.</p></div>';
  const titleField = editingProject
    ? ""
    : '<div class="field studio-title-field"><label>PROJECT TITLE</label><input id="studio-project-title" placeholder="When God Seems Silent" value="' +
      escapeHtml(studioPackState.draftTitle) +
      '"' +
      (readOnly ? " disabled" : "") +
      " /></div>";
  const buttonLabel = editingProject ? "Save Studio Pack Settings" : "Create Production";
  const ready =
    baseItem &&
    baseItem.pack.availability &&
    baseItem.pack.availability.status === "AVAILABLE";
  const cancel = editingProject
    ? '<button class="btn" id="studio-cancel-edit">Cancel</button>'
    : "";

  panel.innerHTML =
    '<div class="studio-creator-head">' +
    header +
    '<div class="studio-mode-tabs">' +
    ["basic", "customize", "advanced"]
      .map(function (candidate) {
        return (
          '<button type="button" class="studio-mode-tab ' +
          (mode === candidate ? "active" : "") +
          '" data-studio-mode="' +
          candidate +
          '">' +
          statusLabel(candidate) +
          "</button>"
        );
      })
      .join("") +
    "</div></div>" +
    titleField +
    '<div class="field"><label>STUDIO PACK</label>' +
    studioPackPickerMarkup(selectedId) +
    "</div>" +
    '<div class="studio-mode-content">' +
    detail +
    "</div>" +
    '<div class="actions studio-create-actions"><button class="btn primary" id="studio-create-or-save"' +
    (readOnly || !ready ? " disabled" : "") +
    ">" +
    buttonLabel +
    "</button>" +
    cancel +
    "</div>";

  const titleInput = document.getElementById("studio-project-title");
  if (titleInput) {
    titleInput.oninput = function () {
      studioPackState.draftTitle = titleInput.value;
    };
  }

  document.querySelectorAll(".studio-mode-tab").forEach(function (button) {
    button.onclick = function () {
      studioPackState.mode = button.dataset.studioMode;
      renderStudioPackCreator(projects, readOnly);
    };
  });

  document.querySelectorAll(".studio-pack-card").forEach(function (button) {
    button.onclick = function () {
      studioPackState.selectedPackId = button.dataset.packId;
      studioPackState.editingPackItemId = null;
      renderStudioPackCreator(projects, readOnly);
    };
  });

  const cancelButton = document.getElementById("studio-cancel-edit");
  if (cancelButton) {
    cancelButton.onclick = function () {
      studioPackState.editingProjectId = null;
      studioPackState.editingPackItemId = null;
      studioPackState.mode = "basic";
      renderStudioPackCreator(projects, readOnly);
    };
  }

  const action = document.getElementById("studio-create-or-save");
  if (action) {
    action.onclick = async function () {
      if (!baseItem) return;
      const overrides = collectStudioPackOverrides(baseItem);
      if (editingProject) {
        render(
          await call("update_project_studio_pack", {
            projectId: editingProject.project.id,
            basePackId: baseItem.pack.id,
            overrides: overrides,
          }),
        );
        showToast("Studio Pack settings saved through the canonical resolver.");
        return;
      }

      const title = (studioPackState.draftTitle || "").trim();
      if (!title) {
        showToast("Enter a project title first.");
        return;
      }
      studioPackState.draftTitle = "";
      render(
        await call("create_project_from_studio_pack", {
          title: title,
          packId: baseItem.pack.id,
          overrides: overrides,
        }),
      );
      showToast("Production created from the resolved Studio Pack.");
    };
  }
}

function renderReviewCenter(projects, readOnly) {
  const panel = document.getElementById("review-center");
  if (!panel) return;
  const review = studioPackState.review;
  if (!review) {
    panel.innerHTML =
      '<p class="eyebrow">REVIEW CENTER</p><h3>Checking canonical exceptions…</h3>';
    return;
  }

  const items = Array.isArray(review.items) ? review.items : [];
  const rows = items.length
    ? items
        .map(function (item) {
          const action =
            item.action && item.action.kind === "retry_job"
              ? '<button class="btn review-retry" data-job-id="' +
                escapeHtml(item.action.job_id) +
                '"' +
                (readOnly ? " disabled" : "") +
                ">Prepare Retry</button>"
              : "";
          return (
            '<article class="review-item ' +
            escapeHtml(String(item.severity || "").toLowerCase()) +
            '"><div class="review-item-head"><div><span>' +
            escapeHtml(statusLabel(item.kind)) +
            "</span><strong>" +
            escapeHtml(item.project_title) +
            "</strong></div><span>" +
            escapeHtml(statusLabel(item.severity)) +
            "</span></div><p>" +
            escapeHtml(item.reason) +
            '</p><div class="review-item-source"><code>' +
            escapeHtml(item.canonical_source) +
            "</code><span>" +
            escapeHtml(item.source_id) +
            "</span></div>" +
            action +
            "</article>"
          );
        })
        .join("")
    : '<div class="review-clear"><strong>No actionable exceptions.</strong><span>Review Center is reconstructed from canonical state, not stored separately.</span></div>';

  panel.innerHTML =
    '<div class="review-center-head"><div><p class="eyebrow">REVIEW CENTER</p><h3>Exceptions, not busywork</h3></div>' +
    '<div class="review-counts"><strong>' +
    escapeHtml(review.blocking_count) +
    ' blocking</strong><span>' +
    escapeHtml(review.actionable_count) +
    " actionable</span></div></div>" +
    '<p class="muted compact">Aggregated from Studio Pack capability state, WorkflowStep, Job and Attempt records.</p>' +
    '<div class="review-list">' +
    rows +
    "</div>";

  document.querySelectorAll(".review-retry").forEach(function (button) {
    button.onclick = async function () {
      await call("retry_review_job", { jobId: button.dataset.jobId });
      render(await call("list_projects"));
      showToast("Job returned to canonical READY state for retry.");
    };
  });
}

async function loadStudioPackWorkspace(projects, readOnly) {
  try {
    const results = await Promise.all([
      call("studio_pack_catalog"),
      call("review_center"),
    ]);
    studioPackState.catalog = results[0];
    studioPackState.review = results[1];

    if (studioPackState.editingProjectId) {
      const editing = projects.find(function (item) {
        return item.project.id === studioPackState.editingProjectId;
      });
      if (editing && editing.project.studio_pack) {
        const current = studioPackItem(editing.project.studio_pack);
        studioPackState.editingPackItemId = current ? current.pack.id : null;
        studioPackState.selectedPackId = current
          ? studioPackBaseId(current)
          : studioPackState.selectedPackId;
      }
    }

    renderStudioPackCreator(projects, readOnly);
    renderReviewCenter(projects, readOnly);
  } catch (_error) {
    const creator = document.getElementById("studio-pack-creator");
    const review = document.getElementById("review-center");
    if (creator) {
      creator.innerHTML =
        '<p class="eyebrow">STUDIO PACK</p><div class="notice">Could not load the canonical Studio Pack catalog.</div>';
    }
    if (review) {
      review.innerHTML =
        '<p class="eyebrow">REVIEW CENTER</p><div class="notice">Could not reconstruct review state.</div>';
    }
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
  if (
    productionPackState.selectedProjectId &&
    !projectIds.has(productionPackState.selectedProjectId)
  ) {
    productionPackState.selectedProjectId = null;
  }
  Array.from(gpuWorkbenchState.selectedProjectIds).forEach(function (projectId) {
    if (!projectIds.has(projectId)) gpuWorkbenchState.selectedProjectIds.delete(projectId);
  });

  const board = renderProjectKanban(projects, workspace.read_only);

  const readOnlyNotice = workspace.read_only
    ? '<div class="notice">Read-only mode. Project state is visible, but no production data will be changed.</div>'
    : "";

  content.innerHTML =
    '<div class="workspace-grid">' +
    '<section class="panel">' +
    '<p class="eyebrow">PROJECT BOARD</p><h2>Productions</h2>' +
    readOnlyNotice +
    '<div id="studio-pack-creator" class="studio-pack-creator"></div>' +
    board +
    '<div id="review-center" class="review-center"></div>' +
    '<div id="production-pack-panel" class="production-pack-panel"></div>' +
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

  document.querySelectorAll(".project-review-center").forEach(function (button) {
    button.onclick = function () {
      const review = document.getElementById("review-center");
      if (review) review.scrollIntoView({ behavior: "smooth", block: "start" });
    };
  });

  document.querySelectorAll(".studio-pack-project").forEach(function (button) {
    button.onclick = function () {
      const item = projects.find(function (candidate) {
        return candidate.project.id === button.dataset.id;
      });
      if (!item) return;
      studioPackState.editingProjectId = item.project.id;
      studioPackState.editingPackItemId = item.project.studio_pack || null;
      const current = item.project.studio_pack
        ? studioPackItem(item.project.studio_pack)
        : null;
      studioPackState.selectedPackId = current
        ? studioPackBaseId(current)
        : studioPackState.selectedPackId;
      studioPackState.mode = "customize";
      renderStudioPackCreator(projects, workspace.read_only);
      const creator = document.getElementById("studio-pack-creator");
      if (creator) creator.scrollIntoView({ behavior: "smooth", block: "start" });
    };
  });

  document.querySelectorAll(".production-pack-project").forEach(function (button) {
    button.onclick = function () {
      const item = projects.find(function (candidate) {
        return candidate.project.id === button.dataset.id;
      });
      if (item) openProductionPack(item.project, workspace.read_only);
    };
  });

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
  const selectedProductionItem = projects.find(function (item) {
    return item.project.id === productionPackState.selectedProjectId;
  });
  renderProductionPackPanel(
    selectedProductionItem ? selectedProductionItem.project : null,
    workspace.read_only,
    selectedProductionItem
      ? productionPackState.viewByProject.get(selectedProductionItem.project.id) || null
      : null,
  );
  renderGpuWorkbench(gpuWorkbenchState.review, workspace.read_only);
  loadComputeProviderStatus(workspace.read_only);
  loadLlmGatewayPanel();
  loadStudioPackWorkspace(projects, workspace.read_only);
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
