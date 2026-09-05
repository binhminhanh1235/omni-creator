# UX, Studio Packs and Advanced Settings

## UX principle

Plugins are an implementation detail for most users.

The normal user should choose a **Studio Pack**, not wire providers manually.

## Default project creation

```text
New Video

Topic / Script
[................................]

Studio Pack
[ Christian Cinematic ]

Duration
[ 15 min ]

Voice
[ Warm Narrator ]

[ Create Production ]
```

## Three UX layers

### Basic

Default:

- topic/script
- Studio Pack
- duration
- voice
- create

### Customize

Optional high-value controls:

- writing style
- visual style
- voice preset
- thumbnail style
- music style
- automation level

### Advanced

Power-user controls:

- plugin/provider routing
- model/provider selection
- LLMGateway routing
- fallback chains
- quality thresholds
- cache policy
- compute provider
- concurrency
- job details
- export settings
- logs/debugging

## Studio Pack

A Studio Pack composes plugins and presets into a production style.

Example:

```yaml
id: christian-cinematic

voice:
  plugin: omnivoice
  preset: warm-narrator

visual:
  routing:
    literal:
      - local-library
      - pexels
      - generated-image
    emotional:
      - local-library
      - pexels
    conceptual:
      - pexels
      - generated-image

thumbnail:
  plugin: cinematic-thumbnail

export:
  plugin: davinci
```

## Alternative pack

```yaml
id: christian-stick-explainer
extends: christian-cinematic

visual:
  routing:
    literal:
      - stick-figure
    emotional:
      - stick-figure
    conceptual:
      - stick-figure

thumbnail:
  plugin: stick-thumbnail
```

Core workflow remains unchanged.

## Automation level

Suggested user control:

- Assisted
- Balanced
- Autopilot

### Assisted

Review between major stages.

### Balanced

Default. Prepare most stages automatically, then show meaningful review checkpoints.

### Autopilot

Topic to production pack with only exception handling.

## Review Center

User should manage exceptions rather than every successful action.

Example:

```text
VIDEO HEALTH            94 / 100

Script                   OK
Voice                    OK
Visual relevance         2 weak scenes
Copyright provenance     OK
Thumbnail                text crowded
Timeline                 OK

[ Fix Automatically ]
[ Review Issues ]
[ Export to Resolve ]
```

## Project board

Suggested top-level workflow columns:

- IDEAS
- PREPARING
- NEEDS REVIEW
- GPU READY
- GPU RUNNING
- READY TO EDIT
- DONE

Each project card should expose only actionable summary.

Example:

```text
When God Seems Silent
GPU PARTIAL
5 GPU jobs remaining
```

## Prepare for Kaggle

Multi-select projects and run:

```text
[ PREPARE GPU BATCH ]
```

The app resolves all possible non-GPU dependencies.

Then show:

```text
10 projects ready
183 GPU jobs
No missing inputs
Estimated workload 3h 48m

[ Connect Kaggle ]
```

## GPU Preflight

Before consuming GPU time:

- validate text/voice configuration
- validate image prompts
- ensure approvals complete
- ensure cache checked
- ensure model/runtime compatibility
- estimate workload
- warn about unresolved inputs

## Low-friction plugin UX

Plugin Manager belongs in Settings.

```text
Visual
  Pexels             enabled
  Stick Figure       enabled
  Meta Image         missing API key

Voice
  OmniVoice          enabled

Export
  DaVinci            enabled
```

Default users should not need to open this screen.

## Settings schema

Plugins may provide JSON Schema plus UI metadata.

Use presets first. Expose low-level parameters only under Advanced.

## Failure UX

Do not show raw traceback by default.

Example:

```text
Stick Figure Animator failed.

The scene was automatically routed to Pexels.

[Details]
```

## Content-first review

The primary scene review question is:

> Does this visual communicate the narration?

not:

> Is this 24 or 30 fps?


## Data Root UX

The portable workspace feature must remain simple.

### First launch

```text
Where should OmniCreator keep your data?

[ Create New Data Folder ]
[ Use Existing Data Folder ]

All projects, media and production state will live here.
You can move or sync this folder later.
```

If the selected directory already contains `.omnicreator/workspace.json`, OmniCreator automatically recognizes it as an existing workspace.

### Settings

```text
Data & Portability

Data Folder
/Users/.../Google Drive/OmniCreatorData

[ Open in Finder ]
[ Change Data Folder ]
[ Prepare for Device Handoff ]

Workspace
Healthy
Revision 184
Last clean handoff: 2 min ago
```

### Changing Data Root

Provide two actions with clear semantics:

- **Use Existing Data Folder**: rebind this machine to an existing workspace
- **Move Data Folder**: safely relocate the current workspace and update the local pointer

Do not make the user edit paths manually.

### Opening on a new machine

Expected UX:

```text
Select OmniCreator Data Folder
        |
workspace detected
        |
Validating state and files...
        |
2 plugins missing
1 API credential missing

[ Install Missing Plugins ]
[ Configure Credential ]

Projects restored
```

Completed projects and completed steps should be visible immediately even if an optional plugin/credential is missing.

### Cloud-sync safety UX

When the Data Root is inside Google Drive or another synchronized location, show a small informational mode:

```text
Cloud-synced workspace

Safe to switch devices after OmniCreator is closed
and synchronization is complete.
```

If a probable active writer on another device is detected:

```text
This workspace may still be open on "MacBook-A".

[ Open Read Only ]
[ Check Again ]
[ Advanced: Force Open ]
```

Do not expose distributed-lock terminology in the default UI.

### Active project files

For cloud providers that support online-only placeholders, recommend that active project media be available offline before opening DaVinci to avoid playback stalls.

## Production Pack export UX

The project board exposes **Export to Resolve** as the user-facing Phase 9 action. Selecting it opens a Production Pack panel for that project with:

- current canonical export state
- portable `ProductionPackV1` JSON
- **Export Production Pack** / **Regenerate Production Pack**
- logical package location
- verified cache-hit feedback
- canonical Job/Attempt history
- missing-artifact/relink diagnostics using artifact ID + logical URI

The panel is intentionally a controller over core export contracts. It does not duplicate FCPXML/SRT/source-report generation in JavaScript and does not persist a second export-status model.

After restart, the panel reconstructs export history from SQLite and can reload the latest verified portable `production-pack.json` artifact. A failed export stays visibly retryable; retry preserves the previous Attempt. In read-only workspace mode, history remains inspectable while export/regeneration is disabled.

Default diagnostics do not expose machine-local absolute paths. Missing media is described through portable artifact identity and an action to restore/relink the Data Root or source before regenerating.


## Phase 10 Studio Pack v1 contract

Phase 10 P0 freezes a portable core contract with:

- `schema: omnicreator.studio-pack`
- `schema_version: 1`
- a portable pack `id` and display `name`
- optional single-parent `extends`
- explicit overrides for automation level, semantic routes, preset IDs and quality thresholds
- explicit remove sets for inherited route/preset/quality keys

A route is expressed through the existing plugin abstraction rather than provider API fields:

```yaml
schema: omnicreator.studio-pack
schema_version: 1
id: christian-cinematic
name: Christian Cinematic

overrides:
  automation_level: BALANCED
  routes:
    visual.literal:
      targets:
        - plugin_type: visual
          capability: local_asset
        - plugin_type: visual
          capability: stock_video
        - plugin_type: visual
          capability: generated_image
    voice:
      targets:
        - plugin_type: voice
          capability: tts
          plugin_id: omnivoice
          preset: warm-narrator
  presets:
    thumbnail: cinematic
  quality_thresholds:
    visual: 80
```

The ordered target list is the fallback order. `plugin_id` is optional: omitting it means later capability resolution may choose any compatible installed plugin. Provider endpoints, model IDs, credentials, secret values and absolute machine paths are not part of this contract.

### Inheritance semantics

Resolution walks the parent chain from root to selected child.

For each child:

1. requested removals delete inherited keys
2. route/preset/quality entries with the same key replace the inherited entry
3. a provided automation level replaces the inherited level
4. omitted fields inherit unchanged

The default automation level is `BALANCED`. Effective configuration and lineage serialize deterministically.

Cycles, missing parents, duplicate pack IDs, duplicate route targets, conflicting replace/remove operations and malformed portable identifiers are invalid.

### Compatibility behavior

Studio Pack v1 is intentionally strict at the structural boundary:

- missing optional v1 fields use defaults so older/minimal v1 files remain readable
- unknown fields are rejected rather than silently becoming an unreviewed provider/secret escape hatch
- unknown schema names and unsupported schema versions are rejected explicitly
- changing required fields or semantics requires a new schema version

This strictness applies to the portable Studio Pack contract. Plugin-specific low-level settings remain owned by the existing plugin settings/configuration layer and Advanced UX.

### Capability availability

A Studio Pack definition does not make a plugin available.

In particular, `Christian Stick Explainer` must remain unavailable/blocked until a compatible stick-figure visual capability/plugin is actually installed and healthy. Phase 10 must not assume Phase 11 already exists.
