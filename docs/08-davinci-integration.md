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


## Phase 9 P1: path-bearing interchange boundary

FCPXML is a **derived export artifact**, never canonical project state. The canonical input remains the normalized `ProductionPackV1`, whose clip references are artifact IDs plus logical URIs.

The export flow is:

```text
ProductionPackV1
  -> validate + normalize a clone
  -> StateStore::get_artifact(artifact_id)
  -> verify project ownership and logical-URI agreement
  -> ArtifactStore::resolve_artifact_path(...)
  -> verify current physical file + canonical hash
  -> convert the current-machine path to an escaped file URL
  -> deterministic FCPXML serializer
```

Resolved physical paths are not written back into `ProductionPackV1` or SQLite. A missing artifact, cross-project artifact, logical-URI mismatch, missing physical file, or hash mismatch fails before the interchange export is considered successful.

### Compatibility profile

P1 uses a typed exporter profile named `FcpxmlCompatibilityProfileV1::Fcpxml110DaVinci`, which emits FCPXML **1.10**. The profile/version belongs to the exporter configuration layer, not to `ProductionPackV1`.

FCPXML 1.10 is intentionally a conservative interchange target: it has the asset/media-representation model, timeline story elements, lanes, roles, and markers needed by this phase without adding Resolve scripting or editor-specific fields to the portable IR.

### Stable edit layout

The serializer uses one deterministic timeline gap as the parent storyline and anchors semantic tracks using fixed FCPXML lanes:

```text
lane +1  V1 Background
lane +2  V2 Primary Visual
lane +3  V3 B-roll
lane +4  V4 Generated Overlays
lane +5  V5 Typography / Scripture

lane -1  A1 Narration
lane -2  A2 Music
lane -3  A3 Ambience
lane -4  A4 SFX
```

FCPXML video/audio roles and clip names carry the same semantic intent. This mapping is exporter-only metadata; the portable timeline contract is unchanged.

Clip `timeline_start_ms`, `source_start_ms`, and `duration_ms` are serialized as rational FCPXML times. Timeline markers are exported as frame-duration markers on the parent gap.

### Data Root move/rebind

Moving, copying, or rebinding the Data Root does not require a relink database. Regeneration performs the same canonical artifact lookup against the new Data Root and emits new file URLs. The portable production pack and SQLite logical media references remain unchanged, and the old machine path is absent from the regenerated interchange.

### DaVinci boundary

The generated FCPXML carries asset locations and the initial edit layout into DaVinci Resolve. After import, DaVinci remains responsible for trimming/refinement, effects, grading, audio mixing, proxies/optimized media, and rendering.

### P1 non-goals

P1 does not add:

- a generic transcoder
- proxy generation
- preview rendering
- NVENC/NVDEC orchestration
- final rendering
- Resolve scripting automation
- a second relink database
- editor-specific resolved paths in canonical IR

## Phase 9 P2: deterministic production package boundary

P2 packages the already-normalized `ProductionPackV1` into a predictable derived production folder without adding editor-specific durable state.

The logical layout is versioned and deterministic:

```text
project://production/<safe-title>-<semantic-hash>/exports/<execution-variant>/
  timeline/
    edit.fcpxml
    subtitles.srt
  reports/
    asset-sources.json
  metadata/
    production-pack.json
```

`<safe-title>` is Unicode-safe filename sanitization. The stable project folder is derived from the normalized production pack plus semantic export inputs. The export variant is an opaque deterministic hash that also includes the current Data Root binding fingerprint because FCPXML contains current-machine file URLs. It is not an absolute path and is not written into the portable ProductionPack metadata.

This variant boundary preserves immutable historical artifacts. After a Data Root move or rebind, the portable semantic hash remains stable while the path-bearing execution variant changes, so cached FCPXML from the old binding cannot be returned. SRT, source metadata and the serialized ProductionPack remain byte-stable when their semantic inputs are unchanged.

### Asset source report

`asset-sources.json` is derived from canonical `Artifact` records. Each entry separates canonical facts from optional source metadata:

- artifact ID
- logical URI
- artifact type
- SHA256
- stable timeline usage references
- portable source/provider/provenance metadata already present in `Artifact.metadata`

Known canonical metadata such as `source_provider`, `source_asset_id`, `selection_ref`, generated-image provider/model/settings and nested `provenance` is copied when present. Missing optional provenance does not fail packaging. The exporter does not invent attribution and rejects machine-specific absolute paths from portable report fields.

### Export state and promotion

Production package generation reuses canonical workflow state:

```text
normalized ProductionPack + export profile + artifact provenance
  -> deterministic semantic hash
  -> current-binding execution hash
  -> Job
  -> Attempt
  -> stage all package components
  -> render/verify SRT + FCPXML + JSON
  -> ArtifactStore multi-output promotion
  -> one SQLite transaction records all Artifacts and marks Attempt/Job SUCCEEDED
```

If generation or promotion fails, the Attempt is marked retryable with `LOCAL_EXPORT_ERROR`; the Job is never reported as successful. No export status database, media database or relink database is introduced.

Verified cache reuse requires the complete four-artifact package from a SUCCEEDED producer job and re-verifies every physical artifact hash before returning a cache hit.

### P2 non-goals

P2 still does not add transcoding, proxy generation, preview rendering, NVENC/NVDEC orchestration, final rendering, Resolve scripting automation, Kafka, Redis, RabbitMQ or Kubernetes.

## Phase 9 P3: desktop export, relink and retry UX

The desktop Production Pack panel is a thin controller over the canonical Phase 9 exporter. It accepts/loads portable `ProductionPackV1` JSON, invokes `ProductionPackageExporterV1`, and derives status/history from the existing SQLite `Job -> Attempt -> Artifact` records. The desktop does not implement a second exporter, export database, relink database or machine-path cache.

The normal flow is:

```text
Project -> Export to Resolve
  -> canonical ProductionPackV1
  -> core exporter/cache lookup
  -> Job / Attempt / Artifact promotion
  -> logical package location + canonical history
```

A successful previous package lets the desktop reload the verified portable `production-pack.json` after restart and offer **Regenerate Production Pack**. Re-running the same execution input after a retryable local export failure reuses the same logical Job and creates a new Attempt, preserving the failed Attempt in history.

Missing-source diagnostics expose portable identity: artifact ID plus logical URI. Machine-specific physical paths used at the export boundary are deliberately not serialized into the desktop diagnostic view. The user can restore/relink the Data Root or source artifact and regenerate. A Data Root copy/rebind still changes the path-bearing execution variant, so regenerated FCPXML resolves the new root and cannot reuse the stale old-root variant.

The desktop may show the logical package location and cache-hit state. It does not add transcoding, preview rendering, proxy generation, final rendering or Resolve scripting automation.
