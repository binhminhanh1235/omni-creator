const invoke = window.__TAURI__.core.invoke;
const content = document.getElementById("content");
const modePill = document.getElementById("mode-pill");
const toast = document.getElementById("toast");

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
  return (
    '<article class="project-card">' +
    "<div>" +
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

function renderWorkspace(snapshot) {
  const workspace = snapshot.workspace;
  const projects = snapshot.projects;
  setMode(workspace.read_only ? "READ ONLY" : "WRITER", workspace.read_only ? "read-only" : "writer");

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
    '<div class="project-list">' + cards + "</div></section>" +
    '<aside class="panel">' +
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
    "</div></aside></div>";

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
