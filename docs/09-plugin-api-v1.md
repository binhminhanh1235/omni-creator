# Plugin API v1

Status: **frozen v1 contract** as of 2026-09-04.

The canonical compatibility anchors are the v1 fixtures under
`crates/omnicreator-core/tests/fixtures/contracts/v1/`. Any change to this
spec must preserve those fixtures unless the affected schema/API version is
explicitly advanced.

## Goals

- language-neutral
- process-isolated
- simple enough for small plugins
- usable locally and remotely
- capability-based
- versioned
- core-owned state

## Manifest

Minimum fields:

```yaml
schema: omnicreator.plugin-manifest
schema_version: 1
id: stick-figure
name: Stick Figure Animator
version: 1.0.0
api_version: 1

types:
  - visual

entrypoint:
  command: python
  args:
    - plugin.py

capabilities:
  - animation
  - conceptual_visual

scene_types:
  - conceptual
  - educational

permissions:
  filesystem:
    - job-workspace
  network: []

settings:
  schema: settings.schema.json
```

## Transport

Local v1:

- JSON Lines
- stdin request
- stdout response/events
- stderr diagnostics

Remote adapters may use HTTP/WebSocket while preserving the logical request/response schema.

## Core methods

All plugins:

- `plugin.initialize`
- `plugin.health`
- `plugin.capabilities`
- `plugin.execute`
- `plugin.cancel`
- `plugin.shutdown`

Capability-specific convenience methods may be added only if they provide clear value.

## Request envelope

```json
{
  "api_version": 1,
  "request_id": "req_123",
  "method": "plugin.execute",
  "params": {
    "operation": "visual.resolve",
    "payload": {}
  }
}
```

## Response envelope

Success:

```json
{
  "api_version": 1,
  "request_id": "req_123",
  "result": {}
}
```

Failure:

```json
{
  "api_version": 1,
  "request_id": "req_123",
  "error": {
    "code": "NETWORK_TIMEOUT",
    "message": "Provider request timed out",
    "retryable": true
  }
}
```

## Progress event

Long-running plugins should emit progress.

```json
{
  "api_version": 1,
  "event": "progress",
  "request_id": "req_123",
  "progress": {
    "percent": 42,
    "message": "Generating frame 84/200"
  }
}
```

## Workspace

Core creates an isolated job workspace:

```text
jobs/<job-id>/
├── input/
├── output/
└── temp/
```

Plugins should write only to granted workspace paths unless a permission explicitly allows more.

Core later promotes verified outputs into the artifact store.

## Core ownership rule

Plugins may not:

- modify SQLite directly
- rewrite project.json
- change selected takes/assets
- mark jobs successful in core state
- bypass artifact verification

They return results. Core commits state.

## Visual resolve input

Primary input is SceneIntent.

Output is one or more VisualCandidate records.

## Visual generate input

Generated visual plugins receive:

- SceneIntent
- selected style/preset
- duration/resolution hints
- seed/settings when applicable

Output references one or more workspace files plus metadata.


### Generated still additive v1 contract

Phase 8 freezes generated still images as an additive use of the existing Plugin API v1 envelope:

```json
{
  "api_version": 1,
  "request_id": "req_123",
  "method": "plugin.execute",
  "params": {
    "operation": "visual.generate",
    "payload": {
      "schema": "omnicreator.generated-image-request",
      "version": 1,
      "scene": {},
      "prompt": "...",
      "negative_prompt": "...",
      "style": {"preset": "..."},
      "resolution": {"width": 1280, "height": 720},
      "aspect_ratio": "16:9",
      "seed": 42,
      "settings": {},
      "prompt_sha256": "...",
      "settings_fingerprint": "..."
    }
  }
}
```

The `scene` member is the frozen provider-neutral `SceneIntentV1`. Provider IDs, model IDs, API endpoint fields and credentials must not be added to SceneIntent. Core resolves execution/provider/model choices outside SceneIntent and performs preflight before expensive execution.

A successful generated-still result returns a workspace-relative output plus verifiable metadata:

```json
{
  "relative_output": "generated/SC01.svg",
  "mime_type": "image/svg+xml",
  "width": 1280,
  "height": 720,
  "sha256": "...",
  "model_id": "reference-svg",
  "model_version": "1",
  "seed": 42,
  "prompt_sha256": "...",
  "settings_fingerprint": "...",
  "metadata": {},
  "provenance": {}
}
```

The plugin never promotes this file itself. Core verifies that the file is a regular file inside the granted output workspace, recomputes its hash, promotes it through ArtifactStore, and commits canonical job/attempt/artifact state. Durable provenance must not contain secret values or machine-specific absolute paths.

Generated-image plugins that can use Phase 7 GPU batching declare the existing `resources` object in their manifest, including a provider-neutral `model_group`. No image-specific scheduler or Kaggle-specific field is introduced.

### Generated-image execution target routing

Phase 8 P2A keeps execution routing outside `SceneIntentV1` and outside the frozen Plugin API v1 envelope. Core resolves one provider-neutral target:

- `local_plugin` for an installed process-isolated plugin with a ready local runtime/configuration,
- `api` for a plugin that additively advertises the `api_execution` capability, has explicit network permission, and whose machine-local API configuration/credential is ready,
- `compute_provider` only when GPU execution was explicitly requested and the existing Phase 5–7 `GpuQueueEligibilityV1` result is `GPU_READY` with a canonical device selection.

The target resolver consumes readiness facts, not secret values. API credential readiness is represented only as `not_required`, `available`, or `missing`; secret material remains outside portable state. Existing generated-image plugins do not need to add `api_execution` and continue to resolve to local execution without a manifest/schema version change.

A GPU request does not silently fall back to API/local execution when canonical ComputeProvider readiness is blocked. The caller must resolve the blocking GPU preflight/reconciliation condition or explicitly prepare a non-GPU execution request. This prevents expensive work from starting on an unintended target and preserves the reviewed Phase 7 scheduling decision.

### ComputeProvider generated-image execution

Phase 8 P2C does not add a generated-image transport or scheduler. Once the P2A decision is `compute_provider`, core maps `GeneratedImagePreparationV1` into the canonical `GpuJobPreparationV1` and dispatches the semantic `visual.generate` payload through the existing `ComputeJobDispatchV1` contract.

Remote generated-image work therefore reuses the Phase 5–7 provider/device selection, remote journal, artifact transfer, reconnect/reconciliation, worker-loss and retry semantics. Independent GPU devices remain independently selected resources; VRAM is never pooled.

Remote artifacts are not trusted on arrival. Core transfers them into staging, verifies the declared SHA-256 and metadata, promotes them through `ArtifactStore`, then commits canonical Job/Attempt/Artifact state. Hash mismatch or corruption cannot produce a successful attempt. Retry keeps the logical job ID and appends a new attempt, while restart/reconnect may recover a completed remote artifact before regeneration.

This remains an internal execution bridge. It adds no provider-specific field to `SceneIntentV1`, no Kaggle-specific generated-image contract, and no Plugin API v1 wire-format change.

## Resource declarations

Plugins should expose resource requirements.

Example:

```yaml
resources:
  gpu: required
  min_vram_mb: 12000
  model_group: flux-schnell
  parallelizable: true
  cost_metric: megapixels
```

This supports ComputeProvider routing and batch planning.

## Retry semantics

Errors should include:

- stable error code
- retryable boolean
- optional retry-after hint
- optional suggested fallback

Core owns retry policy.

## API compatibility

Plugin API v1 is frozen.

- Plugin manifests use `schema: omnicreator.plugin-manifest` with `schema_version: 1`.
- Manifest, request, response and progress envelopes use `api_version: 1`.
- Core accepts API major version `1` and rejects other API versions with a clear incompatibility error.
- Additive optional fields may be introduced within v1 only when existing v1 fixtures remain valid and their established semantics do not change.
- Removing or renaming required fields, changing enum/wire values, or changing the meaning of an existing field requires a new schema/API version.
- Canonical v1 JSON fixtures are compatibility anchors and are exercised by deterministic serialization tests.
- Plugins must not infer compatibility from the OmniCreator application version. Compatibility is determined by the declared schema/API versions.

## SDK goal

A minimal Python plugin should require very little boilerplate.

Target conceptual API:

```python
from omnicreator_plugin import VisualPlugin

class StickPlugin(VisualPlugin):
    def resolve(self, scene):
        ...

StickPlugin().run()
```

SDK convenience must not hide the underlying versioned protocol.
