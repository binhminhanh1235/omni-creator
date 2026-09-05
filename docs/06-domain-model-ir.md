# Domain Model and Project IR

## Goal

OmniCreator modules and plugins should exchange stable structured objects rather than ad-hoc strings.

The internal representation (IR) is the contract that allows providers and exporters to change without rewriting workflows.

## Core entities

Recommended first-class entities:

- Project
- ChannelProfile
- StudioPack
- CreativeBrief
- Script
- Segment
- SceneIntent
- VisualCandidate
- Asset
- VoiceTake
- Timeline
- TimelineItem
- Artifact
- Job
- Attempt
- PluginDescriptor
- Workspace

## Project

A Project aggregates production state and references artifacts.

Important fields:

```text
id
title
created_at
updated_at
studio_pack
channel_profile
status (derived/display)
script_version
production_lock
```

## Segment

Narration should be divided into production-friendly units.

```json
{
  "id": "S04",
  "order": 4,
  "text": "...",
  "voice_direction": {
    "tone": "warm",
    "pace": "slower",
    "tags": []
  }
}
```

## SceneIntent

The semantic bridge between content and visual providers.

Suggested fields:

```json
{
  "id": "SC17",
  "segment_id": "S04",
  "narration": "...",
  "purpose": "...",
  "scene_type": "conceptual",
  "emotion_before": "uncertainty",
  "emotion_after": "cautious hope",
  "duration_hint": 11.5,
  "visual_ideas": [],
  "search_queries": [],
  "avoid": [],
  "continuity": {},
  "aspect_ratio": "16:9"
}
```

## VisualCandidate

Candidate returned before final selection/download/generation.

Fields may include:

```text
provider
provider_asset_id
preview
download variants
semantic score
emotion score
continuity score
freshness score
overall score
explanation
provenance preview
```

## Asset

Normalized media used by the timeline.

```json
{
  "asset_id": "A182",
  "type": "VIDEO",
  "uri": "project://video/SC17.mp4",
  "source_provider": "pexels",
  "width": 1920,
  "height": 1080,
  "duration": 14.2,
  "sha256": "...",
  "provenance": {}
}
```

## Artifact

Artifact is broader than media.

Artifact types include:

- audio
- image
- video
- JSON analysis
- subtitle
- FCPXML
- thumbnail
- report

Artifact should record:

```text
artifact_id
type
uri/path
sha256
size
producer_job
created_at
metadata
```

Timeline references artifact IDs/URIs rather than fragile random filesystem paths.

## VoiceTake

Keep multiple generated takes.

```json
{
  "take_id": "VT_S04_003",
  "segment_id": "S04",
  "artifact_id": "AUDIO_221",
  "provider": "omnivoice",
  "model_version": "v3.2",
  "voice_version": "warm-v4",
  "selected": true
}
```

## Timeline

Internal Timeline should be provider/exporter-neutral.

Example tracks:

```text
V5 Typography / Scripture
V4 Generated overlays
V3 B-roll
V2 Primary visual
V1 Background

A1 Narration
A2 Music
A3 Ambience
A4 SFX
```

MVP does not need every track, but the IR should allow them.

### Production Pack v1

The Phase 9 portable timeline contract is `omnicreator.production-pack` version 1.

Timeline clips persist:

```text
clip_id
artifact_id
logical uri
timeline_start_ms
source_start_ms
duration_ms
optional label
```

They do not persist resolved filesystem paths. Tracks use stable semantic roles, while subtitles and markers use millisecond timeline positions. Canonical normalization sorts tracks, clips, subtitle cues and markers deterministically before export/hashing.

Path-bearing interchange such as FCPXML is an export artifact derived from this portable IR. It must be regenerated after Data Root rebinding rather than treated as canonical project state.

## Provenance

External assets should record enough source data to support future auditing.

Typical fields:

```text
source_provider
source_id
source_url
creator
license
license_checked_at
downloaded_at
original_filename
sha256
usage history
```

## Content-addressed cache

Cache key should include:

- operation/plugin
- plugin version
- model version
- normalized inputs
- relevant settings

Never reuse output across materially different versions.

## Serialization

JSON is preferred for durable interchange/specs in v1.

SQLite stores normalized operational state.

Project export can include human-readable JSON snapshots for debugging/recovery.

## Migration/versioning

All durable structures should carry schema versions.

Example:

```json
{
  "schema": "omnicreator.project",
  "version": 1
}
```

Future migrations should be explicit rather than silently changing persisted semantics.


## Workspace and Data Root

Workspace is the portable top-level container for all durable OmniCreator creator data.

Suggested identity fields:

```text
workspace_id
schema_version
revision
created_at
updated_at
required_plugins
default_channel_profile
```

The actual filesystem path is intentionally **not** a durable Workspace field.

Each machine binds a local Data Root path to the same `workspace_id`.

## Logical URI model

Durable domain objects must reference files using portable logical URIs.

Recommended schemes:

```text
workspace://projects/P001/script/script.md
project://audio/S04.wav
artifact://AUDIO_221
library://assets/PEXELS_12345.mp4
```

Rules:

- never serialize a user home directory into canonical project state
- use forward-slash normalized logical paths
- resolve to OS-native paths only at I/O/export boundaries
- validate resolved paths cannot escape the Data Root
- timeline IR references artifact IDs whenever possible

## External files

Default UX should import/copy external media into the Data Root.

An Advanced **External Reference** mode may exist later, but it must display a portability warning because the referenced file may not exist on another machine.

## Portable configuration

Workspace-level portable configuration:

- Studio Packs
- channel profiles
- project defaults
- plugin settings
- plugin version requirements
- visual rules
- export presets that are path-independent

Machine-level configuration:

- Data Root binding
- device ID
- secrets/keychain references
- installed runtimes
- DaVinci application path
- compute device preferences

## Secret references

Canonical project/workspace JSON may store a symbolic credential reference:

```text
credential_ref: pexels/default
```

It must not store plaintext API keys by default.

When opening the workspace on another machine, missing credential references become setup requirements, not corrupted project state.
