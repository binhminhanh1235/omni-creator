# Plugin API v1 Draft

Status: design draft, not yet frozen.

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

Manifest declares `api_version`.

OmniCreator should support a defined compatibility window and reject incompatible plugins with a clear message.

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
