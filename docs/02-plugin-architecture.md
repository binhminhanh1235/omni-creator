# Plugin Architecture

## Goal

OmniCreator must support changing production styles without changing the core.

Example visual evolution:

```text
today:
SceneIntent -> Pexels

later:
SceneIntent -> Stick Figure Animator

future:
SceneIntent -> Whiteboard / Illustration / Video AI
```

## Core rule

**Core owns state. Plugins own capabilities.**

Plugins must not directly edit project state or project database rows.

A plugin receives a request and returns:

- candidates
- artifacts
- structured metadata
- errors/status

The core decides what is selected, persisted, cached and connected to downstream steps.

## Initial plugin capability types

Keep the list intentionally small:

- VoiceProvider
- VisualProvider
- MusicProvider
- ThumbnailProvider
- CaptionProvider
- QualityProvider
- Exporter
- ComputeProvider

Do not turn infrastructure such as SQLite, cache or job scheduling into plugins.

## SceneIntent as the visual contract

All visual providers receive the same semantic input.

Example:

```json
{
  "scene_id": "SC17",
  "narration": "Forgiveness does not automatically restore trust.",
  "purpose": "show that trust may need rebuilding",
  "visual_mode": "conceptual",
  "emotion": "cautious hope",
  "duration": 11.5,
  "aspect_ratio": "16:9",
  "ideas": [
    "repairing a broken bridge",
    "carefully rebuilding a fence"
  ],
  "avoid": [
    "generic praying hands",
    "church silhouette"
  ]
}
```

Pexels can map this to search queries.

A stick-figure provider can map it to characters, actions and props.

The core remains unchanged.

## Standard visual output

All providers normalize to VisualCandidate and Asset.

Example Asset:

```json
{
  "asset_id": "ASSET_818",
  "scene_id": "SC17",
  "type": "VIDEO",
  "path": "project://video/SC17.mp4",
  "duration": 11.5,
  "width": 1920,
  "height": 1080,
  "provider": "stick-figure",
  "provenance": {
    "source": "generated",
    "license": "project-owned-output"
  }
}
```

## Plugin isolation

Do not use Rust dynamic libraries as the primary plugin mechanism.

Preferred model:

```text
OmniCreator
    |
JSON protocol
    |
plugin process
```

Benefits:

- plugin can be Rust, Python, Go, JS, Java, etc.
- crashes stay isolated
- versioning is simpler
- remote adapters can use the same concepts
- AI plugins can stay in Python

## Local plugin protocol

The simplest v1 transport is JSON lines over stdin/stdout.

Request:

```json
{
  "request_id": "req_123",
  "method": "visual.resolve",
  "params": {
    "scene": {}
  }
}
```

Response:

```json
{
  "request_id": "req_123",
  "result": {
    "candidates": []
  }
}
```

Remote providers can expose equivalent HTTP endpoints.

## Plugin manifest

Example:

```yaml
id: pexels
name: Pexels Visuals
version: 1.0.0
api_version: 1

types:
  - visual

entrypoint:
  command: pexels-plugin

capabilities:
  - stock_video
  - stock_image

scene_types:
  - literal
  - emotional
  - environment

permissions:
  network:
    - api.pexels.com
  filesystem:
    - job-workspace

settings:
  schema: settings.schema.json
```

## Capability routing

Plugins advertise capabilities. The router selects the best implementation based on:

- scene type
- Studio Pack
- capability match
- expected quality
- cost
- availability
- fallback policy

Example:

```text
literal scene
  -> Pexels preferred

conceptual scene
  -> Stick Figure preferred

historical biblical scene
  -> Illustration provider preferred
```

## Fallback chains

A Studio Pack may define:

```text
Local Library
    -> Pexels
        -> Pixabay
            -> Generated Image
```

A result should only be accepted automatically when it reaches the configured quality threshold.

Finding an asset is not the same as finding a good asset.

## Plugin settings

Each plugin may provide JSON Schema.

OmniCreator auto-generates settings UI.

Settings should support visibility levels:

- basic
- advanced

Default users should see only meaningful presets.

### Settings schema subset in Runtime v1

Plugin Runtime v1 intentionally supports a small deterministic JSON Schema subset for auto-generated settings UI.

Supported root shape:

- `type: "object"`
- optional `title` / `description`
- optional `required: string[]`
- optional `properties`

Supported property types:

- `string`
- `integer`
- `number`
- `boolean`

Supported property hints:

- `title`
- `description`
- `default`
- non-empty `enum`
- `minimum` / `maximum` for numeric fields
- `x-omnicreator-visibility: "basic" | "advanced"`

Unsupported property types are skipped with diagnostics rather than crashing the runtime. Invalid schema files likewise produce a fail-soft diagnostic so other plugins remain usable.

The schema path is resolved relative to the plugin directory. Absolute paths, parent traversal and symlink escapes are rejected.

This subset is a UI-description contract, not a claim to implement the entire JSON Schema specification.

## Plugin health lifecycle

Minimum lifecycle:

- initialize
- health
- capabilities
- execute
- cancel
- shutdown

User-facing status:

```text
Pexels          READY
OmniVoice       KAGGLE OFFLINE
Meta Image      API KEY MISSING
Stick Figure    READY
```

## Plugin packaging roadmap

### V1
- local `plugins/` directory
- manifest validation
- executable/process protocol

### V2 — Phase 14
- Plugin Manager local lifecycle
- installed inventory from the canonical PluginRegistry
- built-in + machine-local user-installed roots
- enable/disable as machine-local lifecycle state
- local package/folder install with pre-activation manifest validation
- P1 local install copies into a hidden machine-local staging directory, rejects symlinks/special filesystem entries, validates through the canonical scanner, then atomically renames into the managed user plugin root
- P1 uninstall is restricted to managed user-installed plugin directories and uses a hidden tombstone/rollback boundary; built-in plugins are not removable
- install inspection never executes the plugin entrypoint or arbitrary package scripts
- P2 local updates apply only to user-installed plugins, require the same plugin ID, a valid strictly-newer SemVer version and frozen Plugin API v1 compatibility
- P2 stages the candidate under the machine-local user plugin root, atomically swaps the installed directory, verifies post-activation discovery and restores the previous directory if activation verification fails
- P2 capability-impact preview simulates enabled providers after disable/remove/update and projects affected/blocking Studio Packs and active projects without mutating portable Project or Studio Pack state
- configuration + permission review
- rollback-safe updates

V2 must not create a second plugin registry. Portable Studio Pack/project data keeps capability requirements only; install source, trust, enabled state, staging paths and update availability stay machine-local. A disabled plugin remains discoverable inventory state and is projected as runtime-unavailable.

### Later
- signatures
- compatibility checks
- optional marketplace

A marketplace is explicitly not required for MVP.
