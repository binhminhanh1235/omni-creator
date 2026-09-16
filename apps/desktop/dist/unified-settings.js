(() => {
  const SETTINGS_BUTTON_ID = "unified-settings-button";
  const SETTINGS_MODAL_ID = "unified-settings-modal";
  const SOURCE_CLASS = "p20-settings-source";
  const sourceRegistry = new Map();
  let sourceSequence = 0;
  let contentObserver = null;
  let omniObserver = null;
  let reconcileQueued = false;
  let activeSection = "connections";

  function scheduleReconcile() {
    if (reconcileQueued) return;
    reconcileQueued = true;
    queueMicrotask(() => {
      reconcileQueued = false;
      reconcile();
    });
  }

  function installStyles() {
    if (document.getElementById("p20-unified-settings-styles")) return;
    const style = document.createElement("style");
    style.id = "p20-unified-settings-styles";
    style.textContent = `
      #${SETTINGS_BUTTON_ID} {
        margin-left: auto;
        border: 1px solid rgba(148, 163, 184, .32);
        border-radius: 10px;
        padding: 9px 13px;
        background: rgba(30, 41, 59, .9);
        color: inherit;
        cursor: pointer;
        font-weight: 700;
      }
      #${SETTINGS_BUTTON_ID}[hidden] { display: none !important; }
      #omnivoice-studio-runtime-button { display: none !important; }
      #${SETTINGS_MODAL_ID}[hidden] { display: none !important; }
      #${SETTINGS_MODAL_ID} {
        position: fixed;
        inset: 0;
        z-index: 1500;
        display: grid;
        place-items: center;
        padding: 22px;
        background: rgba(4, 9, 18, .78);
      }
      #${SETTINGS_MODAL_ID} .p20-settings-dialog {
        width: min(1180px, calc(100vw - 32px));
        max-height: calc(100vh - 44px);
        display: grid;
        grid-template-rows: auto minmax(0, 1fr);
        overflow: hidden;
        border: 1px solid rgba(148, 163, 184, .28);
        border-radius: 20px;
        background: var(--surface, #111827);
        color: var(--text, #f8fafc);
        box-shadow: 0 28px 90px rgba(0, 0, 0, .48);
      }
      #${SETTINGS_MODAL_ID} .p20-settings-head {
        display: flex;
        align-items: start;
        justify-content: space-between;
        gap: 18px;
        padding: 20px 22px 16px;
        border-bottom: 1px solid rgba(148, 163, 184, .18);
      }
      #${SETTINGS_MODAL_ID} .p20-settings-head h2 { margin: 2px 0 5px; }
      #${SETTINGS_MODAL_ID} .p20-settings-head p { margin: 0; color: #94a3b8; }
      #${SETTINGS_MODAL_ID} .p20-settings-close {
        border: 1px solid rgba(148, 163, 184, .32);
        border-radius: 10px;
        padding: 9px 12px;
        background: rgba(30, 41, 59, .9);
        color: inherit;
        cursor: pointer;
      }
      #${SETTINGS_MODAL_ID} .p20-settings-layout {
        min-height: 0;
        display: grid;
        grid-template-columns: 190px minmax(0, 1fr);
      }
      #${SETTINGS_MODAL_ID} .p20-settings-nav {
        display: grid;
        align-content: start;
        gap: 8px;
        padding: 18px 14px;
        border-right: 1px solid rgba(148, 163, 184, .16);
        background: rgba(15, 23, 42, .42);
      }
      #${SETTINGS_MODAL_ID} .p20-settings-tab {
        width: 100%;
        border: 1px solid transparent;
        border-radius: 10px;
        padding: 10px 12px;
        text-align: left;
        background: transparent;
        color: #cbd5e1;
        cursor: pointer;
        font-weight: 700;
      }
      #${SETTINGS_MODAL_ID} .p20-settings-tab[aria-selected="true"] {
        border-color: rgba(129, 140, 248, .42);
        background: rgba(99, 102, 241, .16);
        color: #f8fafc;
      }
      #${SETTINGS_MODAL_ID} .p20-settings-content {
        min-width: 0;
        min-height: 0;
        overflow: auto;
        padding: 20px;
      }
      #${SETTINGS_MODAL_ID} .p20-settings-section[hidden] { display: none !important; }
      #${SETTINGS_MODAL_ID} .p20-settings-section { display: grid; gap: 18px; }
      #${SETTINGS_MODAL_ID} .p20-settings-section-head h3 { margin: 0 0 5px; }
      #${SETTINGS_MODAL_ID} .p20-settings-section-head p { margin: 0; color: #94a3b8; line-height: 1.5; }
      #${SETTINGS_MODAL_ID} .p20-settings-host { min-width: 0; }
      #${SETTINGS_MODAL_ID} .p20-settings-host > .panel,
      #${SETTINGS_MODAL_ID} .p20-settings-host > .plugin-manager-panel,
      #${SETTINGS_MODAL_ID} .p20-settings-host > .ov-card {
        margin: 0;
        width: auto;
        max-width: none;
      }
      .${SOURCE_CLASS} { display: none !important; }
      .sidebar-stack.p20-settings-source-stack { display: none !important; }
      .workspace-grid.p20-settings-detached { grid-template-columns: minmax(0, 1fr) !important; }
      #${SETTINGS_MODAL_ID} .p20-empty {
        padding: 16px;
        border: 1px dashed rgba(148, 163, 184, .28);
        border-radius: 12px;
        color: #94a3b8;
      }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-card {
        box-sizing: border-box;
        border: 1px solid rgba(148, 163, 184, .25);
        border-radius: 16px;
        padding: 18px;
        background: rgba(15, 23, 42, .46);
      }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-head {
        display: flex;
        align-items: start;
        justify-content: space-between;
        gap: 16px;
      }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-head h2 { margin: 0 0 6px; font-size: 20px; }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-note { margin: 0; color: #94a3b8; line-height: 1.5; }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-form { display: grid; gap: 14px; margin-top: 18px; }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice label { display: grid; gap: 6px; font-weight: 650; }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice input {
        width: 100%;
        box-sizing: border-box;
        border: 1px solid rgba(148, 163, 184, .35);
        border-radius: 10px;
        padding: 10px 12px;
        background: rgba(15, 23, 42, .72);
        color: inherit;
      }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-actions { display: flex; flex-wrap: wrap; gap: 10px; }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice button {
        border: 1px solid rgba(148, 163, 184, .32);
        border-radius: 10px;
        padding: 9px 12px;
        background: rgba(30, 41, 59, .9);
        color: inherit;
        cursor: pointer;
      }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice button:disabled { opacity: .55; cursor: progress; }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-status { display: grid; gap: 10px; margin-top: 18px; }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-state { font-size: 12px; font-weight: 800; letter-spacing: .08em; }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-state[data-state="ready"] { color: #86efac; }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-state[data-state="needs_api_token"] { color: #fde68a; }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-state[data-state="offline"],
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-state[data-state="degraded"] { color: #fca5a5; }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-grid {
        display: grid;
        grid-template-columns: 150px minmax(0, 1fr);
        gap: 8px 12px;
      }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-grid > span:nth-child(odd) { color: #94a3b8; }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice a { color: #7dd3fc; overflow-wrap: anywhere; }
      #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-machine {
        margin-top: 16px;
        padding: 12px;
        border-radius: 10px;
        background: rgba(15, 23, 42, .55);
      }
      #${SETTINGS_MODAL_ID} [data-p20-source-id="omnivoice-runtime-close"] { display: none !important; }
      @media (max-width: 760px) {
        #${SETTINGS_MODAL_ID} { padding: 10px; }
        #${SETTINGS_MODAL_ID} .p20-settings-dialog { width: calc(100vw - 20px); max-height: calc(100vh - 20px); }
        #${SETTINGS_MODAL_ID} .p20-settings-layout { grid-template-columns: 1fr; }
        #${SETTINGS_MODAL_ID} .p20-settings-nav {
          grid-template-columns: repeat(3, minmax(0, 1fr));
          border-right: 0;
          border-bottom: 1px solid rgba(148, 163, 184, .16);
        }
        #${SETTINGS_MODAL_ID} .p20-settings-tab { text-align: center; }
        #${SETTINGS_MODAL_ID} .p20-settings-omnivoice .ov-grid { grid-template-columns: 1fr; }
      }
    `;
    document.head.appendChild(style);
  }

  function installShell() {
    if (document.getElementById(SETTINGS_MODAL_ID)) return;
    const modal = document.createElement("section");
    modal.id = SETTINGS_MODAL_ID;
    modal.hidden = true;
    modal.setAttribute("aria-label", "OmniCreator settings");
    modal.innerHTML = `
      <div class="p20-settings-dialog" role="dialog" aria-modal="true" aria-labelledby="p20-settings-title">
        <div class="p20-settings-head">
          <div>
            <div class="eyebrow">OMNICREATOR</div>
            <h2 id="p20-settings-title">Settings</h2>
            <p>Workspace, creator intelligence, runtime connections and plugins in one place.</p>
          </div>
          <button class="p20-settings-close" id="p20-settings-close" type="button">Close</button>
        </div>
        <div class="p20-settings-layout">
          <nav class="p20-settings-nav" aria-label="Settings sections">
            <button class="p20-settings-tab" type="button" data-settings-section="general">Workspace</button>
            <button class="p20-settings-tab" type="button" data-settings-section="connections">AI &amp; Runtime</button>
            <button class="p20-settings-tab" type="button" data-settings-section="plugins">Plugins</button>
          </nav>
          <div class="p20-settings-content">
            <section class="p20-settings-section" data-settings-panel="general">
              <div class="p20-settings-section-head">
                <h3>Workspace</h3>
                <p>Data Folder, device handoff and portability controls. Canonical project data stays in the Data Root.</p>
              </div>
              <div id="p20-settings-workspace" class="p20-settings-host"></div>
            </section>
            <section class="p20-settings-section" data-settings-panel="connections">
              <div class="p20-settings-section-head">
                <h3>AI &amp; Runtime</h3>
                <p>Machine-local LLM Provider and OmniVoiceStudio endpoints. Secret values remain outside the Data Root.</p>
              </div>
              <div id="p20-settings-llm" class="p20-settings-host"></div>
              <div id="p20-settings-omnivoice" class="p20-settings-host p20-settings-omnivoice"></div>
            </section>
            <section class="p20-settings-section" data-settings-panel="plugins">
              <div class="p20-settings-section-head">
                <h3>Plugins</h3>
                <p>Built-in plugins ship with OmniCreator. Provider credentials are still configured on this machine only.</p>
              </div>
              <div id="p20-settings-plugins" class="p20-settings-host"></div>
            </section>
          </div>
        </div>
      </div>
    `;
    document.body.appendChild(modal);

    modal.addEventListener("click", (event) => {
      if (event.target === modal) closeSettings();
    });
    document.getElementById("p20-settings-close")?.addEventListener("click", closeSettings);
    modal.querySelectorAll("[data-settings-section]").forEach((button) => {
      button.addEventListener("click", () => selectSection(button.dataset.settingsSection));
    });
    modal.addEventListener("input", proxyControlEvent, true);
    modal.addEventListener("change", proxyControlEvent, true);
    modal.addEventListener("click", proxyClick, true);
    selectSection(activeSection);
  }

  function installButton() {
    if (document.getElementById(SETTINGS_BUTTON_ID)) return;
    const topbar = document.querySelector(".topbar");
    if (!topbar) return;
    const button = document.createElement("button");
    button.id = SETTINGS_BUTTON_ID;
    button.type = "button";
    button.textContent = "Settings";
    button.hidden = true;
    button.addEventListener("click", () => openSettings("connections"));
    topbar.insertBefore(button, document.getElementById("mode-pill"));
  }

  function selectSection(section) {
    const modal = document.getElementById(SETTINGS_MODAL_ID);
    if (!modal) return;
    const allowed = new Set(["general", "connections", "plugins"]);
    activeSection = allowed.has(section) ? section : "connections";
    modal.querySelectorAll("[data-settings-section]").forEach((button) => {
      button.setAttribute("aria-selected", String(button.dataset.settingsSection === activeSection));
    });
    modal.querySelectorAll("[data-settings-panel]").forEach((panel) => {
      panel.hidden = panel.dataset.settingsPanel !== activeSection;
    });
  }

  function openSettings(section) {
    reconcile();
    const modal = document.getElementById(SETTINGS_MODAL_ID);
    if (!modal) return;
    selectSection(section || activeSection);
    modal.hidden = false;
    if (activeSection === "connections") {
      setTimeout(() => {
        const sourceRefresh = document.getElementById("omnivoice-runtime-refresh");
        if (sourceRefresh && !sourceRefresh.closest(`#${SETTINGS_MODAL_ID}`)) sourceRefresh.click();
      }, 0);
    }
  }

  function closeSettings() {
    const modal = document.getElementById(SETTINGS_MODAL_ID);
    if (modal) modal.hidden = true;
  }

  function workspaceSource() {
    const stack = document.querySelector("#content .sidebar-stack");
    if (!stack) return null;
    return Array.from(stack.querySelectorAll(":scope > aside.panel")).find((panel) => {
      const eyebrow = panel.querySelector(".eyebrow");
      return eyebrow && eyebrow.textContent.trim() === "DATA & PORTABILITY";
    }) || null;
  }

  function decorateInteractive(sourceRoot) {
    if (!sourceRoot) return;
    sourceRoot.querySelectorAll("button, input, select, textarea, a").forEach((node) => {
      let key = node.dataset.p20SourceKey;
      if (!key) {
        sourceSequence += 1;
        key = `p20-source-${sourceSequence}`;
        node.dataset.p20SourceKey = key;
      }
      if (node.id && node.dataset.p20SourceId !== node.id) node.dataset.p20SourceId = node.id;
      sourceRegistry.set(key, node);
    });
  }

  function removeCloneIds(clone) {
    if (clone.id) clone.removeAttribute("id");
    clone.querySelectorAll("[id]").forEach((node) => node.removeAttribute("id"));
    clone.querySelectorAll("label[for]").forEach((label) => label.removeAttribute("for"));
  }

  function syncCloneControl(cloneControl) {
    const source = sourceRegistry.get(cloneControl.dataset.p20SourceKey || "");
    if (!source) return;
    if (cloneControl instanceof HTMLInputElement) {
      cloneControl.value = source.value;
      cloneControl.checked = source.checked;
      cloneControl.disabled = source.disabled;
    } else if (cloneControl instanceof HTMLSelectElement || cloneControl instanceof HTMLTextAreaElement) {
      cloneControl.value = source.value;
      cloneControl.disabled = source.disabled;
    } else if (cloneControl instanceof HTMLButtonElement) {
      cloneControl.disabled = source.disabled;
    }
  }

  function mirrorInto(hostId, sourceRoot, emptyText) {
    const host = document.getElementById(hostId);
    if (!host) return;
    host.replaceChildren();
    if (!sourceRoot) {
      const empty = document.createElement("div");
      empty.className = "p20-empty";
      empty.textContent = emptyText;
      host.appendChild(empty);
      return;
    }
    decorateInteractive(sourceRoot);
    const clone = sourceRoot.cloneNode(true);
    clone.classList.remove(SOURCE_CLASS);
    removeCloneIds(clone);
    clone.querySelectorAll("[data-p20-source-key]").forEach(syncCloneControl);
    host.appendChild(clone);
  }

  function setSourceVisibility(workspace, llm, plugins) {
    document.querySelectorAll(`#content .${SOURCE_CLASS}`).forEach((node) => {
      if (node !== workspace && node !== llm && node !== plugins) node.classList.remove(SOURCE_CLASS);
    });
    [workspace, llm, plugins].forEach((node) => node?.classList.add(SOURCE_CLASS));

    const stack = document.querySelector("#content .sidebar-stack");
    const grid = document.querySelector("#content .workspace-grid");
    if (stack && workspace && llm) {
      stack.classList.add("p20-settings-source-stack");
      grid?.classList.add("p20-settings-detached");
    } else {
      stack?.classList.remove("p20-settings-source-stack");
      grid?.classList.remove("p20-settings-detached");
    }
  }

  function observeOmniSource(panel) {
    if (!panel || panel.dataset.p20Observed === "true") return;
    if (omniObserver) omniObserver.disconnect();
    omniObserver = new MutationObserver(scheduleReconcile);
    omniObserver.observe(panel, { childList: true, subtree: true, characterData: true, attributes: true });
    panel.dataset.p20Observed = "true";
  }

  function reconcile() {
    installStyles();
    installButton();
    installShell();
    sourceRegistry.clear();

    const workspace = workspaceSource();
    const llm = document.querySelector("#content #llmgateway-panel");
    const plugins = document.querySelector("#content #plugin-manager-panel");
    const omniPanel = document.getElementById("omnivoice-studio-runtime-panel");
    const omniCard = omniPanel?.querySelector(".ov-card") || null;

    mirrorInto("p20-settings-workspace", workspace, "Open a Data Folder to configure workspace settings.");
    mirrorInto("p20-settings-llm", llm, "LLM Provider settings are available after the workspace opens.");
    mirrorInto("p20-settings-plugins", plugins, "Plugin inventory is available after the workspace opens.");
    mirrorInto("p20-settings-omnivoice", omniCard, "OmniVoiceStudio runtime controls are loading.");
    setSourceVisibility(workspace, llm, plugins);
    observeOmniSource(omniPanel);

    const oldVoiceButton = document.getElementById("omnivoice-studio-runtime-button");
    if (oldVoiceButton) oldVoiceButton.hidden = true;

    const settingsButton = document.getElementById(SETTINGS_BUTTON_ID);
    const workspaceReady = Boolean(workspace || llm || plugins);
    if (settingsButton) settingsButton.hidden = !workspaceReady;
    if (!workspaceReady && !document.getElementById(SETTINGS_MODAL_ID)?.hidden) closeSettings();
  }

  function proxyControlEvent(event) {
    const target = event.target.closest?.("[data-p20-source-key]");
    if (!target || !target.closest(`#${SETTINGS_MODAL_ID}`)) return;
    const source = sourceRegistry.get(target.dataset.p20SourceKey || "");
    if (!source || source.tagName !== target.tagName) return;
    if (source instanceof HTMLInputElement && target instanceof HTMLInputElement) {
      source.value = target.value;
      source.checked = target.checked;
    } else if (
      (source instanceof HTMLSelectElement && target instanceof HTMLSelectElement) ||
      (source instanceof HTMLTextAreaElement && target instanceof HTMLTextAreaElement)
    ) {
      source.value = target.value;
    }
    source.dispatchEvent(new Event(event.type, { bubbles: true }));
  }

  function syncAllMirrorControls() {
    const modal = document.getElementById(SETTINGS_MODAL_ID);
    if (!modal) return;
    modal.querySelectorAll("input[data-p20-source-key], select[data-p20-source-key], textarea[data-p20-source-key]").forEach((clone) => {
      const source = sourceRegistry.get(clone.dataset.p20SourceKey || "");
      if (!source) return;
      if (source instanceof HTMLInputElement && clone instanceof HTMLInputElement) {
        source.value = clone.value;
        source.checked = clone.checked;
      } else if (
        (source instanceof HTMLSelectElement && clone instanceof HTMLSelectElement) ||
        (source instanceof HTMLTextAreaElement && clone instanceof HTMLTextAreaElement)
      ) {
        source.value = clone.value;
      }
    });
  }

  function proxyClick(event) {
    const target = event.target.closest?.("[data-p20-source-key]");
    if (!target || !target.closest(`#${SETTINGS_MODAL_ID}`)) return;
    if (target instanceof HTMLAnchorElement) return;
    const source = sourceRegistry.get(target.dataset.p20SourceKey || "");
    if (!source) return;

    const sourceId = target.dataset.p20SourceId || source.id || "";
    if (sourceId === "open-plugin-manager") {
      event.preventDefault();
      event.stopPropagation();
      selectSection("plugins");
      return;
    }

    if (!(target instanceof HTMLButtonElement)) return;
    event.preventDefault();
    event.stopPropagation();
    syncAllMirrorControls();
    if (!source.disabled) source.click();
    setTimeout(scheduleReconcile, 30);
    setTimeout(scheduleReconcile, 350);
  }

  function installDocumentShortcuts() {
    document.addEventListener(
      "click",
      (event) => {
        const llmButton = event.target.closest?.(".review-configure-llm");
        if (!llmButton || llmButton.closest(`#${SETTINGS_MODAL_ID}`)) return;
        openSettings("connections");
        setTimeout(() => {
          const clone = document.querySelector(`#${SETTINGS_MODAL_ID} [data-p20-source-id="llmgateway-url"]`);
          clone?.focus();
        }, 0);
      },
      true,
    );
    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && !document.getElementById(SETTINGS_MODAL_ID)?.hidden) closeSettings();
    });
  }

  function installObservers() {
    const content = document.getElementById("content");
    if (!content || contentObserver) return;
    contentObserver = new MutationObserver(scheduleReconcile);
    contentObserver.observe(content, { childList: true, subtree: true, characterData: true });
  }

  function install() {
    installStyles();
    installButton();
    installShell();
    installDocumentShortcuts();
    installObservers();
    reconcile();
    setTimeout(scheduleReconcile, 50);
    setTimeout(scheduleReconcile, 250);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", install, { once: true });
  } else {
    install();
  }
})();
