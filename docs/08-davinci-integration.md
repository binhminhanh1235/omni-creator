# DaVinci Resolve Integration Boundary

## Product boundary

DaVinci Resolve is the final editing/rendering environment.

OmniCreator should prepare an editable production pack instead of becoming a second video editor.

## OmniCreator owns

- narration files
- selected/generated visuals
- subtitles
- scene timing
- track/timeline metadata
- source/provenance metadata
- thumbnail assets
- optional markers
- project organization
- FCPXML or other supported interchange formats

## DaVinci owns

- editorial decisions after import
- trimming/refinement
- transitions
- effects
- motion graphics
- color grading
- audio mixing
- proxy/optimized media when desired
- final render/encode

## Explicit non-goals

Do not build these into MVP:

- final MP4 rendering
- generic encoding pipeline
- NVENC/NVDEC orchestration
- proxy generator
- preview movie renderer
- color engine
- effects engine
- audio mixer
- full Resolve scripting automation

These features add complexity without improving the main content-preparation value.

## Timeline export strategy

### V1

Generate:

- local production folder
- SRT
- FCPXML / compatible interchange
- asset metadata

### V2

Improve timeline structure:

- named tracks
- bins
- markers
- scripture markers
- review markers
- consistent asset naming

### Later

Evaluate Resolve scripting only after the interchange workflow is reliable.

## Suggested folder layout

```text
MyProject/
├── project.json
├── script/
│   └── script.md
├── audio/
│   ├── S01.wav
│   ├── S02.wav
│   └── ...
├── video/
│   ├── SC001.mp4
│   └── ...
├── images/
├── thumbnail/
├── subtitles/
│   └── captions.srt
├── timeline/
│   └── timeline.fcpxml
└── metadata/
    └── assets.json
```

No default folders for:

- proxy
- transcoded
- preview render
- final render cache

## Resolution policy

Technical media choice follows content choice.

Default should be adaptive rather than forcing 4K or 1080p globally.

- 1080p for ordinary B-roll
- higher resolution when cropping/reframing provides real benefit
- high-resolution generated stills
- avoid unnecessary 4K downloads

## Asset selection and download

Download selected assets directly to the local production/library storage.

Do not route stock video through Kaggle merely to transcode it.

## Production Pack IR v1

Phase 9 P0 freezes a provider/editor-neutral portable timeline contract before any path-bearing interchange is generated.

`ProductionPackV1` contains:

- project identity/title
- rational frame rate
- stable semantic track roles
- timeline clips that reference both canonical artifact IDs and logical URIs
- subtitle cues
- typed markers

Stable track roles are ordered as:

```text
V1 Background
V2 Primary Visual
V3 B-roll
V4 Generated Overlays
V5 Typography / Scripture
A1 Narration
A2 Music
A3 Ambience
A4 SFX
```

The portable contract contains no machine-local absolute media paths. A production pack can therefore survive Data Root move/copy/rebind. FCPXML or another path-bearing interchange is regenerated on the current machine by resolving the same artifact/logical references at the export boundary.

P0 also renders deterministic SRT from ordered, non-overlapping subtitle cues using millisecond timestamps. Invalid or overlapping cue ranges fail before an export artifact is produced.

## Stable references

Internal timeline should reference artifact IDs.

The exporter resolves them to current local paths.

This allows:

- moving project directories
- relinking
- deduplicated/shared asset library
- safer cache/storage changes

## Success criterion

The DaVinci integration is successful when the user opens the imported project/timeline and can immediately begin creative editing rather than file hunting, syncing narration and manually rebuilding scene order.
