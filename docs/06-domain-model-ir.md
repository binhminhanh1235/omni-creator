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
