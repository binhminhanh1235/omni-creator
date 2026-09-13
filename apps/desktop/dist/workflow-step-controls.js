(function () {
  const core = window.__TAURI__ && window.__TAURI__.core;
  if (!core || typeof core.invoke !== "function") return;

  const nativeInvoke = core.invoke.bind(core);

  // Existing creator UI continues to use the stable command name. Route every
  // Start / Resume invocation through the Phase 17 policy-aware backend.
  core.invoke = function (command, args) {
    const mapped =
      command === "start_creator_production"
        ? "start_creator_production_phase17"
        : command;
    return nativeInvoke(mapped, args);
  };

  const stages = [
    { key: "content.prepare", label: "Content" },
    { key: "scene.plan", label: "Scenes" },
    { key: "visual.prepare", label: "Visuals" },
    { key: "voice.prepare", label: "Voice" },
    { key: "production.pack", label: "Production Pack" },
  ];

  function readOnlySession() {
    const pill = document.getElementById("mode-pill");
    if (!pill) return false;
    return (
      pill.classList.contains("read-only") ||
      pill.textContent.toUpperCase().includes("READ ONLY")
    );
  }

  function projectIdForCard(card) {
    const action = card.querySelector(
      ".project-creator-action[data-id], .studio-pack-project[data-id], .production-pack-project[data-id]",
    );
    return action ? action.getAttribute("data-id") : null;
  }

  function explain(enabled) {
    return enabled
      ? "Automatic execution is ON for this stage. Canonical manual/external replacement remains available."
      : "Automatic execution is OFF. This stage is still required and must be satisfied manually/externally, or turned ON later.";
  }

  async function decorateCard(card) {
    if (card.dataset.workflowStepControls === "loading" || card.dataset.workflowStepControls === "ready") {
      return;
    }
    const projectId = projectIdForCard(card);
    const pills = Array.from(card.querySelectorAll(".creator-stage-pill"));
    if (!projectId || pills.length < stages.length) return;

    card.dataset.workflowStepControls = "loading";
    try {
      const policies = await nativeInvoke("workflow_step_execution_policies", {
        projectId: projectId,
      });
      const byKey = new Map(
        (Array.isArray(policies) ? policies : []).map(function (policy) {
          return [policy.step, policy];
        }),
      );
      const readOnly = readOnlySession();

      stages.forEach(function (stage, index) {
        const pill = pills[index];
        const policy = byKey.get(stage.key);
        if (!pill || !policy) return;

        const enabled = policy.automatic_execution_enabled !== false;
        pill.dataset.autoExecution = enabled ? "on" : "off";
        pill.dataset.stepKey = stage.key;

        const label = document.createElement("label");
        label.className = "workflow-step-auto-toggle";
        label.title = explain(enabled);

        const input = document.createElement("input");
        input.type = "checkbox";
        input.checked = enabled;
        input.disabled = readOnly;
        input.setAttribute("aria-label", stage.label + " automatic execution");

        const text = document.createElement("span");
        text.textContent = enabled ? "AUTO ON" : "AUTO OFF";

        input.onchange = async function () {
          const next = input.checked;
          input.disabled = true;
          try {
            const snapshot = await nativeInvoke("set_workflow_step_automatic_execution", {
              stepId: policy.step_id,
              enabled: next,
            });
            if (typeof window.showToast === "function") {
              window.showToast(
                stage.label +
                  (next
                    ? " automatic execution enabled."
                    : " automatic execution disabled. Use manual/external continuation when this stage is required."),
              );
            }
            if (typeof window.render === "function") {
              window.render(snapshot);
            } else {
              input.disabled = readOnlySession();
              text.textContent = next ? "AUTO ON" : "AUTO OFF";
              pill.dataset.autoExecution = next ? "on" : "off";
              label.title = explain(next);
            }
          } catch (_error) {
            input.checked = !next;
            input.disabled = readOnlySession();
            text.textContent = !next ? "AUTO ON" : "AUTO OFF";
          }
        };

        label.appendChild(input);
        label.appendChild(text);
        pill.appendChild(label);
      });
      card.dataset.workflowStepControls = "ready";
    } catch (_error) {
      delete card.dataset.workflowStepControls;
    }
  }

  function decorateAll() {
    document.querySelectorAll(".project-card").forEach(function (card) {
      void decorateCard(card);
    });
  }

  const observer = new MutationObserver(function () {
    decorateAll();
  });

  function startObserver() {
    const root = document.getElementById("content") || document.body;
    observer.observe(root, { childList: true, subtree: true });
    decorateAll();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", startObserver, { once: true });
  } else {
    startObserver();
  }
})();
