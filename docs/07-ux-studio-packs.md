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


## Phase 10 P1 capability-aware catalog

P1 adds a separate portable catalog document without changing the canonical Studio Pack v1 schema:

- `schema: omnicreator.studio-pack-catalog`
- `schema_version: 1`
- ordered canonical serialization is derived by pack ID
- catalog entries remain ordinary `StudioPackV1` definitions
- effective configuration is still resolved by the P0 inheritance resolver

Availability is not stored as portable truth. It is calculated from the resolved pack plus the canonical `PluginRegistry` and an ephemeral runtime readiness snapshot. The runtime snapshot may distinguish ready, setup-required and unavailable plugins, but it is machine-local state and is intentionally not serializable into the Data Root.

P1 status values are:

- `AVAILABLE`: every required route has at least one compatible ready implementation
- `AVAILABLE_WITH_SETUP`: required capabilities are installed but at least one route needs runtime setup such as credentials
- `UNAVAILABLE`: at least one required route has no compatible capability or only unavailable implementations

Machine-readable reasons distinguish missing required capability, missing preferred plugin, plugin unavailable, setup required and unavailable optional fallback. Preferred/fallback misses do not corrupt the pack definition and do not block a route when another compatible target is usable.

Checked-in runtime capability inventory for P1 is intentionally derived from real manifests:

- Pexels: `stock_video`, `stock_image`, `preview_first_search`, `selected_asset_download`
- Generated Image Reference: `generated_still`, `visual_generate`, `deterministic_seed`
- Generated Image API: `generated_still`, `visual_generate`, `api_execution`

There is no stick-figure plugin on current main. `Christian Stick Explainer` therefore requires the explicit semantic capability `stick_figure_visual` and remains unavailable until a compatible visual plugin advertises that capability. Generic generated-image capability is not treated as an equivalent substitute.

## Phase 10 P2 creator controls and Review Center

P2 makes Studio Packs the creator-facing entry point without changing canonical ownership.

### Basic

Basic is the default new-production path. It presents the built-in Studio Pack catalog, derived availability and blocking/setup reasons before project creation. A project can only be created from an `AVAILABLE` pack. The selected resolved pack ID is written directly to the existing canonical `Project.studio_pack` field.

The UI does not need to understand PluginRegistry internals. Availability still comes from the P1 evaluator over the canonical `PluginRegistry` plus an ephemeral machine-local runtime readiness snapshot. Credential readiness is derived from symbolic `*_env` plugin setting references and the local process environment; secret values are never serialized into the portable catalog or project state.

### Customize

Customize exposes only high-value contract fields already owned by Studio Pack v1:

- curated preset IDs
- automation level
- existing quality thresholds

A project customization is represented as an ordinary portable child `StudioPackV1` extending the selected built-in pack. It is saved in the portable Studio Pack catalog and resolved by the existing deterministic P0 resolver. The project continues to store only the selected Studio Pack ID.

The desktop layer does not implement another inheritance algorithm. Resetting all project overrides returns the project to its built-in parent and removes the now-unused project-specific child definition.

### Advanced

Advanced shows resolved plugin/capability routes, target ordering, value sources and capability diagnostics. Existing LLMGateway, Compute Provider/GPU Workbench and Production Pack controls remain in their established machine-local/canonical boundaries.

Creator overrides cannot inject arbitrary routes, provider endpoints, model request fields, credential values or absolute machine paths into Studio Pack state. Provider-specific controls remain in the owning plugin/runtime surface.

### Automation levels

`ASSISTED`, `BALANCED` and `AUTOPILOT` are deterministic policy projections over canonical workflow/review semantics:

- Assisted never auto-advances creator review checkpoints.
- Balanced may auto-advance deterministic/low-risk work and routes ambiguous decisions to review.
- Autopilot may auto-advance low-risk work without an ambiguity checkpoint.
- All three stop on blocking capability/setup errors and high-impact exceptions.

These policies do not create Job/Attempt state and do not bypass the existing workflow state machine.

### Review Center

Review Center is reconstructed on demand from existing canonical state:

- Studio Pack capability/setup availability
- `WorkflowStep`
- `Job`
- `Attempt`

Failed/retryable jobs expose a canonical retry preparation action. The action transitions the existing Job back to `READY` through the existing state-transition rules; it does not edit a UI-only review record. Once the canonical problem is resolved, the item disappears from the next projection.

No Review Center database, JSON state file, browser local storage or shadow workflow state is introduced.
\n## Phase 10 P3 derived Project Kanban\n\nP3 turns the existing derived `ProjectDisplayStatus` into the creator-facing seven-column board:\n\n- `DRAFT` -> `IDEAS`\n- `PREPARING` -> `PREPARING`\n- `NEEDS_REVIEW` -> `NEEDS REVIEW`\n- `GPU_PARTIAL` -> `NEEDS REVIEW`\n- `GPU_READY` -> `GPU READY`\n- `GPU_RUNNING` -> `GPU RUNNING`\n- `READY_FOR_EDIT` -> `READY TO EDIT`\n- `DONE` -> `DONE`\n\n`GPU_PARTIAL` is intentionally folded into `NEEDS REVIEW` because it is an exception/retry state, not a separate top-level creator stage.\n\nBoard placement is never persisted separately. Desktop snapshots reconstruct the column and actionable summary from canonical Project / WorkflowStep / Job state on every load, including restart and read-only opens.\n\nCards expose a next-action summary such as retryable jobs, remaining preparation work, GPU-ready work or the existing Production Pack editing action. Raw provider execution details do not become the default card surface.\n\nExisting Studio Pack settings, Review Center, GPU Workbench and Production Pack panels remain the owning control surfaces. Kanban only organizes projects and links to those actions.\n\n### P3 hardening\n\nP3 regression coverage requires:\n\n- interrupted RUNNING work reconciles through the existing restart contract and reappears as an actionable review card\n- moving the Data Root preserves the same derived board projection\n- read-only workspaces reconstruct board state but cannot mutate project state\n- browser local/session storage is not used as board truth\n- GPU Workbench and Production Pack regressions continue to pass\n