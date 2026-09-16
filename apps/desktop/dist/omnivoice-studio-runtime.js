(() => {
  const PANEL_ID = "omnivoice-studio-runtime-panel";
  const BUTTON_ID = "omnivoice-studio-runtime-button";

  function invoke(command, payload = {}) {
    const fn = window.__TAURI__?.core?.invoke;
    if (typeof fn !== "function") {
      return Promise.reject(new Error("Tauri invoke bridge is unavailable."));
    }
    return fn(command, payload);
  }

  function statusLabel(state) {
    return String(state || "unknown").replaceAll("_", " ").toUpperCase();
  }

  function setText(id, value) {
    const node = document.getElementById(id);
    if (node) node.textContent = value ?? "—";
  }

  function setLink(id, value) {
    const node = document.getElementById(id);
    if (!node) return;
    node.textContent = value ?? "—";
    if (value) {
      node.href = value;
      node.removeAttribute("aria-disabled");
    } else {
      node.removeAttribute("href");
      node.setAttribute("aria-disabled", "true");
    }
  }

  function renderStatus(status) {
    const state = document.getElementById("omnivoice-runtime-state");
    if (state) {
      state.textContent = statusLabel(status?.state);
      state.dataset.state = status?.state || "unknown";
    }
    setText("omnivoice-runtime-message", status?.message);
    setLink("omnivoice-runtime-ui-url", status?.ui_url);
    setLink("omnivoice-runtime-api-url", status?.api_url);
    setLink("omnivoice-runtime-mcp-url", status?.mcp_url);
    setText("omnivoice-runtime-health", status?.health_status || "unknown");
    setText(
      "omnivoice-runtime-generation",
      status?.standalone_audio_generation === true
        ? "available"
        : status?.standalone_audio_generation === false
          ? "unavailable"
          : "unknown",
    );
    setText(
      "omnivoice-runtime-mcp",
      status?.mcp_enabled === true
        ? "enabled"
        : status?.mcp_enabled === false
          ? "disabled"
          : "unknown",
    );
    setText(
      "omnivoice-runtime-credential",
      status?.bearer_token_env
        ? `${status.bearer_token_env}: ${status.credential_present ? "present" : "not present"}`
        : "No bearer-token environment variable configured",
    );

    const base = document.getElementById("omnivoice-runtime-base-url");
    const env = document.getElementById("omnivoice-runtime-token-env");
    if (base && document.activeElement !== base) base.value = status?.base_url || "";
    if (env && document.activeElement !== env) env.value = status?.bearer_token_env || "";
  }

  function showError(error) {
    const message = error?.message || String(error);
    setText("omnivoice-runtime-message", message);
    const state = document.getElementById("omnivoice-runtime-state");
    if (state) {
      state.textContent = "ERROR";
      state.dataset.state = "degraded";
    }
  }

  async function refreshStatus() {
    const button = document.getElementById("omnivoice-runtime-refresh");
    if (button) button.disabled = true;
    try {
      renderStatus(await invoke("omnivoice_studio_status"));
    } catch (error) {
      showError(error);
    } finally {
      if (button) button.disabled = false;
    }
  }

  async function saveSettings(event) {
    event.preventDefault();
    const save = document.getElementById("omnivoice-runtime-save");
    const base = document.getElementById("omnivoice-runtime-base-url")?.value || "";
    const tokenEnv = document.getElementById("omnivoice-runtime-token-env")?.value.trim() || null;
    if (save) save.disabled = true;
    try {
      const status = await invoke("save_omnivoice_studio_settings", {
        baseUrl: base,
        bearerTokenEnv: tokenEnv,
      });
      renderStatus(status);
    } catch (error) {
      showError(error);
    } finally {
      if (save) save.disabled = false;
    }
  }

  function closePanel() {
    const panel = document.getElementById(PANEL_ID);
    if (panel) panel.hidden = true;
  }

  function openPanel() {
    const panel = document.getElementById(PANEL_ID);
    if (!panel) return;
    panel.hidden = false;
    refreshStatus();
    document.getElementById("omnivoice-runtime-base-url")?.focus();
  }

  function installStyles() {
    if (document.getElementById("omnivoice-runtime-styles")) return;
    const style = document.createElement("style");
    style.id = "omnivoice-runtime-styles";
    style.textContent = `
      #${BUTTON_ID} { margin-left: auto; }
      #${PANEL_ID}[hidden] { display: none; }
      #${PANEL_ID} {
        position: fixed; inset: 0; z-index: 1200; display: grid; place-items: center;
        background: rgba(4, 9, 18, 0.72); padding: 24px;
      }
      #${PANEL_ID} .ov-card {
        width: min(760px, calc(100vw - 32px)); max-height: calc(100vh - 48px); overflow: auto;
        background: var(--surface, #111827); color: var(--text, #f8fafc); border: 1px solid rgba(148,163,184,.28);
        border-radius: 18px; padding: 22px; box-shadow: 0 24px 70px rgba(0,0,0,.42);
      }
      #${PANEL_ID} .ov-head { display: flex; align-items: start; justify-content: space-between; gap: 16px; }
      #${PANEL_ID} h2 { margin: 0 0 6px; }
      #${PANEL_ID} .ov-note { margin: 0; color: #94a3b8; line-height: 1.5; }
      #${PANEL_ID} .ov-form { display: grid; gap: 14px; margin-top: 20px; }
      #${PANEL_ID} label { display: grid; gap: 6px; font-weight: 650; }
      #${PANEL_ID} input {
        width: 100%; box-sizing: border-box; border: 1px solid rgba(148,163,184,.35); border-radius: 10px;
        padding: 10px 12px; background: rgba(15,23,42,.72); color: inherit;
      }
      #${PANEL_ID} .ov-actions { display: flex; flex-wrap: wrap; gap: 10px; }
      #${PANEL_ID} button, #${BUTTON_ID} {
        border: 1px solid rgba(148,163,184,.32); border-radius: 10px; padding: 9px 12px;
        background: rgba(30,41,59,.9); color: inherit; cursor: pointer;
      }
      #${PANEL_ID} button:disabled { opacity: .55; cursor: progress; }
      #${PANEL_ID} .ov-status { display: grid; gap: 10px; margin-top: 18px; }
      #${PANEL_ID} .ov-state { font-size: 12px; font-weight: 800; letter-spacing: .08em; }
      #${PANEL_ID} .ov-state[data-state="ready"] { color: #86efac; }
      #${PANEL_ID} .ov-state[data-state="needs_api_token"] { color: #fde68a; }
      #${PANEL_ID} .ov-state[data-state="offline"],
      #${PANEL_ID} .ov-state[data-state="degraded"] { color: #fca5a5; }
      #${PANEL_ID} .ov-grid { display: grid; grid-template-columns: 150px minmax(0, 1fr); gap: 8px 12px; }
      #${PANEL_ID} .ov-grid > span:nth-child(odd) { color: #94a3b8; }
      #${PANEL_ID} a { color: #7dd3fc; overflow-wrap: anywhere; }
      #${PANEL_ID} .ov-machine { margin-top: 16px; padding: 12px; border-radius: 10px; background: rgba(15,23,42,.55); }
      @media (max-width: 640px) { #${PANEL_ID} .ov-grid { grid-template-columns: 1fr; } }
    `;
    document.head.appendChild(style);
  }

  function installPanel() {
    if (document.getElementById(PANEL_ID)) return;
    installStyles();

    const topbar = document.querySelector(".topbar");
    if (topbar && !document.getElementById(BUTTON_ID)) {
      const button = document.createElement("button");
      button.id = BUTTON_ID;
      button.type = "button";
      button.textContent = "OmniVoiceStudio";
      button.addEventListener("click", openPanel);
      topbar.insertBefore(button, document.getElementById("mode-pill"));
    }

    const panel = document.createElement("section");
    panel.id = PANEL_ID;
    panel.hidden = true;
    panel.setAttribute("aria-label", "OmniVoiceStudio runtime settings");
    panel.innerHTML = `
      <div class="ov-card" role="dialog" aria-modal="true" aria-labelledby="omnivoice-runtime-title">
        <div class="ov-head">
          <div>
            <h2 id="omnivoice-runtime-title">OmniVoiceStudio runtime</h2>
            <p class="ov-note">Paste the current Studio root, Public Studio UI, Public REST API, or Public MCP URL. OmniCreator normalizes it to one runtime domain.</p>
          </div>
          <button id="omnivoice-runtime-close" type="button" aria-label="Close">Close</button>
        </div>
        <form class="ov-form" id="omnivoice-runtime-form">
          <label>
            Runtime URL
            <input id="omnivoice-runtime-base-url" type="url" spellcheck="false" placeholder="https://example.trycloudflare.com" required />
          </label>
          <label>
            Bearer token environment variable
            <input id="omnivoice-runtime-token-env" type="text" spellcheck="false" placeholder="OMNIVOICE_API_TOKEN" />
          </label>
          <div class="ov-actions">
            <button id="omnivoice-runtime-save" type="submit">Save &amp; connect</button>
            <button id="omnivoice-runtime-refresh" type="button">Refresh</button>
          </div>
        </form>
        <div class="ov-status">
          <div id="omnivoice-runtime-state" class="ov-state" data-state="unknown">UNKNOWN</div>
          <div id="omnivoice-runtime-message" class="ov-note">Runtime status has not been checked yet.</div>
          <div class="ov-grid">
            <span>Studio UI</span><a id="omnivoice-runtime-ui-url" target="_blank" rel="noreferrer">—</a>
            <span>REST API</span><a id="omnivoice-runtime-api-url" target="_blank" rel="noreferrer">—</a>
            <span>MCP</span><a id="omnivoice-runtime-mcp-url" target="_blank" rel="noreferrer">—</a>
            <span>Health</span><span id="omnivoice-runtime-health">—</span>
            <span>Audio generation</span><span id="omnivoice-runtime-generation">—</span>
            <span>MCP capability</span><span id="omnivoice-runtime-mcp">—</span>
            <span>Credential</span><span id="omnivoice-runtime-credential">—</span>
          </div>
        </div>
        <p class="ov-note ov-machine">Machine-local runtime setting only. The Cloudflare tunnel URL and bearer token value are not written to the portable Data Root or canonical project artifacts.</p>
      </div>
    `;
    document.body.appendChild(panel);

    document.getElementById("omnivoice-runtime-close")?.addEventListener("click", closePanel);
    document.getElementById("omnivoice-runtime-refresh")?.addEventListener("click", refreshStatus);
    document.getElementById("omnivoice-runtime-form")?.addEventListener("submit", saveSettings);
    panel.addEventListener("click", (event) => {
      if (event.target === panel) closePanel();
    });
    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && !panel.hidden) closePanel();
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", installPanel, { once: true });
  } else {
    installPanel();
  }
})();
