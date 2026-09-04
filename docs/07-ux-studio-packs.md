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
