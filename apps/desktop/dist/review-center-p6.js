const reviewRecoveryP6State = {
  loading: false,
  loadedSignature: null,
  inventory: null,
  recoveryByProject: new Map(),
};

function reviewP6ProjectSignature(projects, readOnly) {
  return JSON.stringify({
    readOnly: Boolean(readOnly),
    projects: (projects || []).map(function (item) {
      return {
        id: item.project && item.project.id,
        pack: item.project && item.project.studio_pack,
        steps: (item.steps || []).map(function (step) {
          return [step.step_id, step.step, step.unit, step.status];
        }),
      };
    }),
    review: studioPackState.review
      ? [studioPackState.review.blocking_count, studioPackState.review.actionable_count, (studioPackState.review.items || []).length]
      : null,
  });
}

function reviewP6StepForItem(projectView, item) {
  if (!projectView || !Array.isArray(projectView.steps)) return null;
  if (item.canonical_source === "WorkflowStep") {
    return projectView.steps.find(function (step) {
      return step.step_id === item.source_id;
    }) || null;
  }
  return null;
}

function reviewP6StageFromStepName(stepName) {
  const step = String(stepName || "").toLowerCase();
  if (step === "content.prepare") return "content";
  if (step === "scene.plan") return "scene_plan";
  if (step.startsWith("visual.")) return "visual";
  if (step.startsWith("voice.") || step.includes("tts")) return "voice";
  if (step === "production.pack") return "production_pack";
  if (step.startsWith("export.")) return "export";
  return "other";
}

function reviewP6RouteFromItem(item) {
  if (!item || item.canonical_source !== "PluginRegistry + StudioPack availability") return null;
  const source = String(item.source_id || "");
  const separator = source.indexOf(":");
  return separator >= 0 ? source.slice(separator + 1) : null;
}

function reviewP6Stage(projectView, item) {
  const step = reviewP6StepForItem(projectView, item);
  if (step) return reviewP6StageFromStepName(step.step);
  if (item.action && item.action.kind === "configure_llm_gateway") return "content";
  const route = reviewP6RouteFromItem(item);
  if (route && route.startsWith("visual.")) return "visual";
  if (route && route.startsWith("voice.")) return "voice";
  return "other";
}

function reviewP6BlockerClass(item) {
  if (!item) return "dependency_blocked";
  if (item.action && item.action.kind === "configure_llm_gateway") return "configuration_required";
  switch (String(item.kind || "").toLowerCase()) {
    case "setup_requirement":
      return "configuration_required";
    case "missing_capability":
      return "provider_unavailable";
    case "failed_or_retryable_job":
      return "retryable_execution";
    case "blocked_workflow":
    default:
      return "dependency_blocked";
  }
}

function reviewP6ManualAvailable(stage) {
  return ["content", "scene_plan", "visual", "voice"].includes(stage);
}

function reviewP6ManualLabel(stage) {
  switch (stage) {
    case "content": return "Provide Script Manually";
    case "scene_plan": return "Provide Scene Plan Manually";
    case "visual": return "Provide My Visual";
    case "voice": return "Provide My Audio";
    case "production_pack":
    case "export": return "Repair Manually";
    default: return "Provide Manually";
  }
}

function reviewP6ReplaceLabel(stage) {
  switch (stage) {
    case "visual": return "Replace Visual";
    case "voice": return "Replace Audio / Timing";
    case "production_pack":
    case "export": return "Repair / Replace Result";
    default: return "Replace Result";
  }
}

function reviewP6PackItem(projectView) {
  const packId = projectView && projectView.project && projectView.project.studio_pack;
  const packs = studioPackState.catalog && Array.isArray(studioPackState.catalog.packs)
    ? studioPackState.catalog.packs
    : [];
  return packs.find(function (item) {
    return item.pack && item.pack.id === packId;
  }) || null;
}

function reviewP6RouteContext(projectView, item) {
  const routeKey = reviewP6RouteFromItem(item);
  const pack = reviewP6PackItem(projectView);
  if (!routeKey || !pack || !pack.pack || !Array.isArray(pack.pack.routes)) return null;
  const route = pack.pack.routes.find(function (candidate) {
    return candidate.key === routeKey;
  });
  if (!route) return null;
  const targets = Array.isArray(route.targets) ? route.targets : [];
  const inventory = reviewRecoveryP6State.inventory || { plugins: [], readiness: [] };
  const plugins = Array.isArray(inventory.plugins) ? inventory.plugins : [];
  const readiness = Array.isArray(inventory.readiness) ? inventory.readiness : [];
  const alternatives = [];

  targets.forEach(function (target) {
    plugins.forEach(function (plugin) {
      const id = plugin.id || (plugin.manifest && plugin.manifest.id);
      const types = plugin.types || (plugin.manifest && plugin.manifest.types) || [];
      const capabilities = plugin.capabilities || (plugin.manifest && plugin.manifest.capabilities) || [];
      if (!id) return;
      if (target.plugin_id && target.plugin_id !== id) return;
      if (!types.includes(target.plugin_type) || !capabilities.includes(target.capability)) return;
      const ready = readiness.find(function (entry) { return entry.plugin_id === id; });
      alternatives.push({
        id: id,
        capability: target.capability,
        state: ready ? ready.status : "ready",
      });
    });
  });

  return {
    key: routeKey,
    targets: targets,
    alternatives: alternatives,
    requiredCapability: targets.length ? targets[0].capability : null,
    // Current Studio Pack routes are ordered deterministic fallbacks. Do not fake an explicit choice.
    explicitAlternativeChoice: false,
  };
}

function reviewP6Action(kind, label, primary, enabled, target, metadata) {
  return {
    kind: kind,
    label: label,
    primary: Boolean(primary),
    enabled: Boolean(enabled),
    target: target,
    metadata: metadata || {},
  };
}

function reviewP6ActionsForItem(projectView, item, readOnly) {
  const stage = reviewP6Stage(projectView, item);
  const blockerClass = reviewP6BlockerClass(item);
  const route = reviewP6RouteContext(projectView, item);
  const mutationsEnabled = !readOnly;
  const actions = [];
  const manualPreferred = reviewP6ManualAvailable(stage) &&
    ["provider_unavailable", "configuration_required", "manual_input_required"].includes(blockerClass);

  if (reviewP6ManualAvailable(stage)) {
    actions.push(reviewP6Action(
      "provide_manually",
      reviewP6ManualLabel(stage),
      manualPreferred,
      mutationsEnabled,
      "manual_takeover",
      { stage: stage },
    ));
  }

  const generatedExternal = stage === "visual" && route && route.targets.some(function (target) {
    return target.capability === "generated_still" || target.capability === "stick_figure_visual";
  });
  if (generatedExternal) {
    actions.push(reviewP6Action(
      "provide_manually",
      "Provide External Image",
      false,
      mutationsEnabled,
      "external_handoff",
      { stage: "visual" },
    ));
  }

  if (["configuration_required", "provider_unavailable"].includes(blockerClass)) {
    actions.push(reviewP6Action(
      "configure",
      "Configure",
      !manualPreferred,
      mutationsEnabled,
      item.action && item.action.kind === "configure_llm_gateway" ? "llmgateway" : "plugins",
      { stage: stage },
    ));
  }

  if (route && route.explicitAlternativeChoice) {
    const compatibleReady = route.alternatives.filter(function (candidate) {
      return candidate.state === "ready" && candidate.capability === route.requiredCapability;
    });
    if (compatibleReady.length >= 2) {
      actions.push(reviewP6Action(
        "choose_alternative",
        "Choose Alternative",
        false,
        mutationsEnabled,
        "alternative_provider",
        { route: route.key, capability: route.requiredCapability, candidates: compatibleReady },
      ));
    }
  }

  const retryable = item.action && item.action.kind === "retry_job";
  const setupJob = item.action && item.action.kind === "configure_llm_gateway" && item.canonical_source === "Job / Attempt";
  if ((retryable || setupJob) && !["manual_input_required", "validation_error"].includes(blockerClass)) {
    actions.push(reviewP6Action(
      "retry",
      "Retry",
      actions.every(function (action) { return !action.primary; }),
      mutationsEnabled,
      "retry_job",
      { jobId: item.source_id },
    ));
  }

  actions.push(reviewP6Action("details", "Details", false, true, "details", {}));
  reviewP6EnsureSinglePrimary(actions);
  return { stage: stage, blockerClass: blockerClass, route: route, actions: actions };
}

function reviewP6EnsureSinglePrimary(actions) {
  let found = false;
  actions.forEach(function (action) {
    if (!action.primary) return;
    if (found) action.primary = false;
    else found = true;
  });
  if (!found) {
    const candidate = actions.find(function (action) { return action.kind !== "details"; });
    if (candidate) candidate.primary = true;
  }
}

function reviewP6RecoveryItems(projectView, readOnly) {
  const projectId = projectView.project.id;
  const recovery = reviewRecoveryP6State.recoveryByProject.get(projectId);
  if (!recovery || !Array.isArray(recovery.items)) return [];
  return recovery.items
    .filter(function (item) { return item.state === "missing" || item.state === "invalid"; })
    .map(function (item) {
      const stage = item.kind === "visual" ? "visual" : "voice";
      const actions = [
        reviewP6Action(
          "replace_result",
          reviewP6ReplaceLabel(stage),
          true,
          !readOnly,
          "production_recovery",
          { stage: stage, kind: item.kind, canonicalId: item.canonical_id },
        ),
        reviewP6Action("details", "Details", false, true, "details", {}),
      ];
      return {
        id: "recovery:" + projectId + ":" + item.kind + ":" + item.canonical_id,
        project_id: projectId,
        project_title: projectView.project.title,
        kind: "selected_result_invalid",
        severity: "BLOCKING",
        reason: item.detail || "The selected canonical artifact is missing or invalid.",
        canonical_source: "Job / Attempt / ArtifactStore",
        source_id: item.canonical_id,
        p6: {
          stage: stage,
          blockerClass: "selected_result_invalid",
          resultState: item.state,
          logicalUri: item.logical_uri || null,
          artifactId: item.artifact_id || null,
          actions: actions,
        },
      };
    });
}

function reviewP6ContextForBaseItem(projectView, item, readOnly) {
  const projection = reviewP6ActionsForItem(projectView, item, readOnly);
  return Object.assign({}, item, {
    p6: {
      stage: projection.stage,
      blockerClass: projection.blockerClass,
      resultState: "none",
      route: projection.route,
      actions: projection.actions,
    },
  });
}

function reviewP6ActionMarkup(item, action, index) {
  const classes = ["btn", "review-p6-action"];
  if (action.primary) classes.push("primary");
  const disabled = action.enabled ? "" : " disabled";
  const title = !action.enabled && action.kind !== "details"
    ? ' title="Read-only workspace: this action cannot mutate canonical state."'
    : "";
  return '<button class="' + classes.join(" ") + '" data-review-id="' +
    escapeHtml(item.id) + '" data-action-index="' + index + '"' + disabled + title + '>' +
    escapeHtml(action.label) + '</button>';
}

function reviewP6DetailsMarkup(item) {
  const p6 = item.p6 || {};
  const actions = Array.isArray(p6.actions) ? p6.actions : [];
  const actionNames = actions.map(function (action) { return action.label; }).join(", ");
  const route = p6.route && p6.route.key
    ? '<div><span>ROUTE</span><code>' + escapeHtml(p6.route.key) + '</code></div>'
    : "";
  const result = p6.resultState && p6.resultState !== "none"
    ? '<div><span>RESULT</span><code>' + escapeHtml(p6.resultState) + '</code></div>'
    : "";
  const logical = p6.logicalUri
    ? '<div><span>LOGICAL URI</span><code>' + escapeHtml(p6.logicalUri) + '</code></div>'
    : "";
  return '<details class="review-p6-details" data-details-for="' + escapeHtml(item.id) + '">' +
    '<summary>Canonical details</summary><div class="review-p6-detail-grid">' +
    '<div><span>STAGE</span><code>' + escapeHtml(p6.stage || "other") + '</code></div>' +
    '<div><span>CLASSIFICATION</span><code>' + escapeHtml(p6.blockerClass || "dependency_blocked") + '</code></div>' +
    result + route + logical +
    '<div><span>SOURCE</span><code>' + escapeHtml(item.canonical_source || "canonical state") + '</code></div>' +
    '<div><span>ITEM</span><code>' + escapeHtml(item.source_id || "") + '</code></div>' +
    '<div class="wide"><span>NEXT VALID ACTIONS</span><strong>' + escapeHtml(actionNames || "Details") + '</strong></div>' +
    '</div></details>';
}

function reviewP6CardMarkup(item) {
  const p6 = item.p6 || { actions: [] };
  const primary = p6.actions.find(function (action) { return action.primary; });
  const ordered = [];
  if (primary) ordered.push(primary);
  p6.actions.forEach(function (action) {
    if (action !== primary && action.kind !== "details") ordered.push(action);
  });
  const detailsAction = p6.actions.find(function (action) { return action.kind === "details"; });
  if (detailsAction) ordered.push(detailsAction);

  return '<article class="review-item review-p6-item ' +
    escapeHtml(String(item.severity || "").toLowerCase()) + '">' +
    '<div class="review-item-head"><div><span>' + escapeHtml(statusLabel(item.kind)) +
    '</span><strong>' + escapeHtml(item.project_title) + '</strong></div><span>' +
    escapeHtml(statusLabel(item.severity)) + '</span></div>' +
    '<div class="review-p6-context"><span>' + escapeHtml(statusLabel(p6.stage || "other")) +
    '</span><span>' + escapeHtml(statusLabel(p6.blockerClass || "dependency_blocked")) + '</span></div>' +
    '<p>' + escapeHtml(item.reason) + '</p>' +
    '<div class="review-p6-actions">' + ordered.map(function (action, index) {
      return reviewP6ActionMarkup(item, action, index);
    }).join("") + '</div>' +
    reviewP6DetailsMarkup(item) +
    '</article>';
}

function reviewP6OpenCreator(projectView, readOnly, stage, external) {
  if (!projectView) return;
  creatorRunState.selectedProjectId = projectView.project.id;
  renderCreatorRunPanel(projectView, readOnly);
  const panel = document.getElementById("creator-run-panel");
  if (panel) panel.scrollIntoView({ behavior: "smooth", block: "start" });

  if (stage === "content") {
    const input = document.getElementById("creator-input-text");
    if (input) input.focus();
  } else if (stage === "scene_plan") {
    const button = document.getElementById("creator-open-manual-scenes");
    if (button && !readOnly) button.click();
  } else if (stage === "visual" && !creatorRunState.manualVisualByProject.has(projectView.project.id)) {
    loadCreatorManualVisual(projectView, readOnly);
  } else if (stage === "voice" && !creatorRunState.manualVoiceByProject.has(projectView.project.id)) {
    loadCreatorManualVoice(projectView, readOnly);
  }

  if (external) {
    showToast("External handoff is available inside the canonical manual takeover panel for this stage.");
  }
}

function reviewP6BindAction(projects, readOnly, item, action) {
  const projectView = (projects || []).find(function (candidate) {
    return candidate.project && candidate.project.id === item.project_id;
  });
  switch (action.target) {
    case "retry_job":
      return async function () {
        await call("retry_review_job", { jobId: action.metadata.jobId });
        render(await call("list_projects"));
        showToast("Job returned to canonical READY state for retry.");
      };
    case "llmgateway":
      return function () {
        const panel = document.getElementById("llmgateway-panel");
        if (panel) panel.scrollIntoView({ behavior: "smooth", block: "start" });
      };
    case "plugins":
      return function () {
        const panel = document.getElementById("plugin-manager-panel");
        if (panel) panel.scrollIntoView({ behavior: "smooth", block: "start" });
        else {
          const studio = document.getElementById("studio-pack-creator");
          if (studio) studio.scrollIntoView({ behavior: "smooth", block: "start" });
        }
      };
    case "manual_takeover":
      return function () {
        reviewP6OpenCreator(projectView, readOnly, action.metadata.stage, false);
      };
    case "external_handoff":
      return function () {
        reviewP6OpenCreator(projectView, readOnly, action.metadata.stage, true);
      };
    case "production_recovery":
      return function () {
        if (!projectView) return;
        openProductionPack(projectView.project, readOnly);
        const panel = document.getElementById("production-pack-panel");
        if (panel) panel.scrollIntoView({ behavior: "smooth", block: "start" });
      };
    case "details":
      return function () {
        const details = document.querySelector('[data-details-for="' + CSS.escape(item.id) + '"]');
        if (details) details.open = !details.open;
      };
    default:
      return function () {};
  }
}

function reviewP6BindActions(projects, readOnly, items) {
  document.querySelectorAll(".review-p6-action").forEach(function (button) {
    const item = items.find(function (candidate) { return candidate.id === button.dataset.reviewId; });
    if (!item) return;
    const primary = item.p6.actions.find(function (action) { return action.primary; });
    const ordered = [];
    if (primary) ordered.push(primary);
    item.p6.actions.forEach(function (action) {
      if (action !== primary && action.kind !== "details") ordered.push(action);
    });
    const details = item.p6.actions.find(function (action) { return action.kind === "details"; });
    if (details) ordered.push(details);
    const action = ordered[Number(button.dataset.actionIndex)];
    if (!action) return;
    button.onclick = reviewP6BindAction(projects, readOnly, item, action);
  });
}

async function reviewP6LoadCanonicalContext(projects, readOnly, signature) {
  if (reviewRecoveryP6State.loading) return;
  reviewRecoveryP6State.loading = true;
  try {
    try {
      reviewRecoveryP6State.inventory = await invoke("plugin_inventory");
    } catch (_error) {
      reviewRecoveryP6State.inventory = null;
    }

    const recoveryRows = await Promise.all((projects || []).map(async function (projectView) {
      const steps = projectView.steps || [];
      const contentReady = steps.some(function (step) {
        return step.step === "content.prepare" && step.unit === "project" && step.status === "SUCCEEDED";
      });
      const sceneReady = steps.some(function (step) {
        return step.step === "scene.plan" && step.unit === "project" && step.status === "SUCCEEDED";
      });
      if (!contentReady || !sceneReady) return [projectView.project.id, null];
      try {
        return [projectView.project.id, await invoke("production_recovery_status", { projectId: projectView.project.id })];
      } catch (_error) {
        return [projectView.project.id, null];
      }
    }));
    reviewRecoveryP6State.recoveryByProject = new Map(recoveryRows.filter(function (row) { return row[1]; }));
    reviewRecoveryP6State.loadedSignature = signature;
  } finally {
    reviewRecoveryP6State.loading = false;
  }
  renderReviewCenter(projects, readOnly);
}

function renderReviewCenter(projects, readOnly) {
  const panel = document.getElementById("review-center");
  if (!panel) return;
  const review = studioPackState.review;
  if (!review) {
    panel.innerHTML = '<p class="eyebrow">REVIEW CENTER</p><h3>Checking canonical exceptions…</h3>';
    return;
  }

  const signature = reviewP6ProjectSignature(projects, readOnly);
  if (reviewRecoveryP6State.loadedSignature !== signature && !reviewRecoveryP6State.loading) {
    reviewP6LoadCanonicalContext(projects, readOnly, signature);
  }

  const projectById = new Map((projects || []).map(function (item) {
    return [item.project.id, item];
  }));
  const baseItems = (Array.isArray(review.items) ? review.items : []).map(function (item) {
    return reviewP6ContextForBaseItem(projectById.get(item.project_id), item, readOnly);
  });
  const recoveryItems = [];
  (projects || []).forEach(function (projectView) {
    recoveryItems.push.apply(recoveryItems, reviewP6RecoveryItems(projectView, readOnly));
  });
  const items = baseItems.concat(recoveryItems).sort(function (left, right) {
    const rank = { BLOCKING: 3, ACTION_REQUIRED: 2, INFO: 1 };
    return (rank[right.severity] || 0) - (rank[left.severity] || 0) ||
      String(left.project_title).localeCompare(String(right.project_title)) ||
      String(left.id).localeCompare(String(right.id));
  });
  const blockingCount = items.filter(function (item) { return item.severity === "BLOCKING"; }).length;
  const actionableCount = items.filter(function (item) {
    return item.p6.actions.some(function (action) { return action.kind !== "details" && action.enabled; });
  }).length;

  const rows = items.length
    ? items.map(reviewP6CardMarkup).join("")
    : '<div class="review-clear"><strong>No actionable exceptions.</strong><span>Review Center is reconstructed from canonical state, not stored separately.</span></div>';

  panel.innerHTML =
    '<div class="review-center-head"><div><p class="eyebrow">REVIEW CENTER</p><h3>Universal recovery</h3></div>' +
    '<div class="review-counts"><strong>' + escapeHtml(blockingCount) + ' blocking</strong><span>' +
    escapeHtml(actionableCount) + ' actionable</span></div></div>' +
    '<p class="muted compact">Canonical blockers are projected into only the recovery actions that make sense now. Manual takeover, external handoff and repair reuse the verified Phase 16 controllers.</p>' +
    (readOnly ? '<div class="notice subtle">Read-only workspace: Details remains available; every mutating recovery action is disabled.</div>' : '') +
    (reviewRecoveryP6State.loading ? '<div class="notice subtle">Refreshing provider readiness and artifact health from canonical state…</div>' : '') +
    '<div class="review-list">' + rows + '</div>';

  reviewP6BindActions(projects, readOnly, items);
}
