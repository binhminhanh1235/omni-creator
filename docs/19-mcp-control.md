# Phase 18 P2 — MCP stdio control surface

Tracking: Phase 18 umbrella #99, P2 #102.

## Boundary

OmniCreator MCP is a transport over `omnicreator-application`. It does not own a scheduler, workflow database, artifact truth, SQL mutation path, or provider-specific shadow state.

```text
MCP client
   |
stdio / MCP 2026-07-28
   |
omnicreator mcp serve
   |
omnicreator-application
   |
omnicreator-core + canonical SQLite + ArtifactStore
```

The same `ApplicationControlService`, `CreatorRunControlServiceV1`, writer lease, read-only guard, Job/Attempt contracts, dependency invalidation, ArtifactStore verification, manual/external takeover, and ProductionPack recovery semantics remain authoritative.

## Runtime and protocol

P2 targets the stable MCP revision `2026-07-28` through the official Rust SDK `rmcp 3.3.0`.

`rmcp 3.3.0` requires Rust 1.88, so the workspace MSRV is intentionally raised from 1.80 to 1.88. This keeps OmniCreator on the official protocol implementation rather than introducing a private MCP parser/transport.

Local P2 transport is stdio only. Remote Streamable HTTP remains deferred until authentication/authorization is designed and verified.

## One executable

MCP is exposed by the existing Rust executable:

```text
omnicreator --data-root <path> [--read-only] [--device-id <id>] \
  [--llm-provider-config <path> | --llmgateway-config <path>] \
  [--studio-pack-catalog <path>] mcp serve
```

`--llm-provider-config` is the P3 provider-neutral machine-local config. `--llmgateway-config` remains a legacy compatibility option; the two are mutually exclusive. Secret values stay environment-backed and are never passed in MCP arguments or protocol payloads.

There is no Python or Node control-plane sidecar. In MCP mode stdout is reserved for protocol frames. Startup/runtime errors go to stderr.

## Tool surface

Inspection:

- `workspace_status`
- `projects_list`
- `project_get`
- `workflow_status`
- `creator_state`
- `review_list`
- `runtime_status`
- `visual_control` with `status`
- `voice_control` with `status`
- `production_control` with `status` or `recovery`

`runtime_status` can inspect the combined snapshot or the plugin, compute, or LLM runtime specifically. LLM inspection returns sanitized provider/readiness metadata only, never the credential value or authorization header.

Mutation/control:

- `project_create`
- `project_update` for rename, Studio Pack binding/clearing, or delete; delete is destructive and requires explicit `confirm: true`
- `workflow_set_step_auto`
- `creator_start_or_resume`
- `content_control` for manual provide/import
- `scene_plan_control` for editor/provide/import
- `visual_control` for manual, asset-library, stock-selection, generated approval, and external handoff/result paths
- `voice_control` for manual voice/timing and external handoff/result paths
- `production_control` for assemble, rebuild/export, and recovery repairs

The grouped `*_control` tools deliberately keep the public MCP surface capability-oriented while their payloads map to existing typed application contracts.

## Structured results and errors

Successful tools return MCP `structuredContent` containing the same serialized application response/projection used by other transports.

Application failures are tool errors, not transport failures. They return `structuredContent` using:

```json
{
  "schema": "omnicreator.mcp-error",
  "version": 1,
  "error": {
    "schema": "omnicreator.control-error",
    "version": 1,
    "code": "read_only",
    "message": "the active workspace session is read-only"
  }
}
```

Canonical error categories remain `read_only`, `writer_conflict`, `not_found`, `invalid_input`, `invalid_transition`, `blocked`, `capability_unavailable`, `provider_unavailable`, `artifact_invalid`, `stale_input`, and `internal`.

Responses and error messages sanitize the active Data Root and Bearer markers before crossing the MCP boundary.

## Writer, destructive actions, and read-only behavior

Writable tool calls acquire the existing `WorkspaceSession` writer lease. MCP never bypasses single-writer semantics.

Project deletion is the destructive P2 project operation and is guarded separately from writer access. `project_update` with `action: "delete"` rejects the request with typed `invalid_input` unless `confirm: true` is supplied. The safety regression verifies that an unconfirmed delete leaves the project intact and that a confirmed delete removes it canonically.

With `--read-only`, inspection uses `Workspace::inspect` + `ApplicationControlService::for_read_only`. Any mutation reaches the same application-layer guard and returns typed `read_only`.

## Creator automatic runtime

`creator_start_or_resume` delegates to `CreatorRunControlServiceV1::start_or_resume_v1`.

With P3, MCP automatic Content/Scene execution uses the same configured provider-neutral runtime as CLI/Desktop. `--llm-provider-config` can select LLMGateway, OpenRouter, or a supported generic OpenAI-compatible provider. The legacy `--llmgateway-config` path is adapted into that same shared contract.

If no LLM provider is configured, the tool reports typed `provider_unavailable` without a hidden network attempt. Provider selection never becomes MCP-owned workflow state.

Automatic visual and voice/compute execution are not fabricated in P2/P3. When machine-local plugin/compute execution is not wired into this process, MCP reports `capability_unavailable`. Canonical manual/external takeover remains available and can complete the project through ProductionPack/export.

Provider-neutral LLM configuration, OpenRouter protocol behavior and secret boundaries are documented in `docs/20-llm-providers.md`.

## Resources and prompts

P2 does not add duplicate MCP resources or prompt templates. The typed inspection tools already expose the canonical sanitized snapshots needed by clients. Resources/prompts may be added later only when they reduce context cost or add a distinct capability without becoming a second source of truth.

## P2 verification gate

P2 is DONE / VERIFIED with PR #109, exact-head CI #526 / run `34805856829`, guarded squash merge `026871cdeeaae61f69da4c7bde76c19742b01842`, and post-merge CI #527 / run `34806301978`.

Verified properties include:

1. Official MCP client connects to the `omnicreator` child process over stdio and discovers the typed tools.
2. A full provider-free manual MCP path toggles AUTO OFF, supplies canonical Content/ScenePlan/visual/voice, reads Review Center/recovery, and reaches ProductionPack + Resolve export.
3. MCP project projection equals the direct Application Control Service projection for the same canonical state.
4. A read-only MCP process rejects mutation with structured `read_only`.
5. Destructive project delete requires explicit confirmation; an unconfirmed request is rejected without changing canonical state.
6. Creator Start/Resume reports a typed provider blocker without Desktop or a hidden network fallback when no LLM provider is configured.
7. MCP results do not leak Data Root/API-key/Bearer markers in the covered projections/errors.
8. Existing CLI, Rust, Plugins, and Desktop regressions remain green after the Rust 1.88 MSRV bump.

P3 extends the verified P2 transport with provider-neutral LLM runtime configuration and sanitized LLM readiness; it does not add a second MCP workflow engine, remote MCP HTTP, or P4/P5 behavior.
