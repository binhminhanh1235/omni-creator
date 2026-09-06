# OmniCreator Roadmap

This roadmap prioritizes **maximum creator value with minimum unnecessary infrastructure**.

## Phase 0 - Freeze the contracts

Before broad implementation:

1. define Project IR v1
2. define SceneIntent v1
3. define Asset / Artifact v1
4. define Job / Attempt states
5. define Plugin Manifest v1
6. define Plugin API transport/envelopes
7. define ComputeProvider capability model
8. define Workspace/Data Root manifest v1
9. define logical URI/path resolver contract
10. define device-handoff and clean-snapshot contract

Deliverable:
- schemas/specs stable enough to build against

## Phase 1 - Local Core

Build the lightweight local control plane.

- Rust workspace
- Tauri desktop shell
- user-selectable Data Root
- Create New / Use Existing Data Folder flow
- portable workspace manifest
- logical URI/path resolver
- SQLite persistence inside the portable workspace
- single-writer workspace lock/lease
- clean handoff snapshots + rotating backups
- project CRUD
- artifact store
- hashes/cache
- workflow DAG
- step/job/attempt state machine
- dependency invalidation
- resumable local execution

Success:
- app restart does not lose state
- changing one segment invalidates only affected downstream work
- copying/syncing the Data Root to another machine and rebinding restores projects and step states
- no canonical project/artifact record depends on the old machine's absolute paths

## Phase 2 - LLMGateway integration

- provider adapter
- script/content task calls
- structured-output contracts
- SceneIntent generation
- visual search-query generation
- quality/reasoning calls

Success:
- core never hardcodes individual LLM providers

## Phase 3 - Plugin Runtime v1

- scan `plugins/`
- manifest validation
- process lifecycle
- JSONL protocol
- health checks
- permissions/workspace basics
- auto-generated settings from JSON Schema

Initial plugins/adapters:
- Pexels VisualProvider
- OmniVoice VoiceProvider
- DaVinci Exporter

## Phase 4 - Pexels and Scene Intelligence

- SceneIntent pipeline
- literal/emotional/conceptual classification
- multiple search queries per scene
- preview-first candidate workflow
- content relevance scoring
- cliché penalty
- reuse penalty
- direct selected-asset download
- provenance metadata

Success:
- user reviews a small number of highly relevant candidates rather than random stock search results

## Phase 5 - Kaggle ComputeProvider

- connect/disconnect worker
- T4 x2 capability discovery
- GPU-ready queue
- immediate artifact sync
- remote JSONL/journal reconciliation
- worker-loss recovery
- retry
- model grouping/affinity
- batch estimates

Success:
- Kaggle session can disappear without losing completed local work

## Phase 6 - OmniVoice Burst Production

- segment-level TTS jobs
- preflight
- pronunciation/normalization lock
- parallel T4 workers where beneficial
- take history
- retry per segment
- cache by input/model/voice hash
- timing artifacts

Success:
- many projects can be prepared first and narrated in one burst session

## Phase 7 - GPU Batch Workbench

- multi-project GPU READY selection
- Prepare GPU Batch
- preflight
- workload estimate
- weekly GPU budget view
- model-group schedule
- Burst Mode
- remaining/retry queue

Success:
- Kaggle wall-clock time is dominated by useful GPU work

## Phase 8 - Generated Image Plugin

- first image provider(s)
- fallback from stock to generated still
- thumbnail background generation
- provider-neutral local/API/ComputeProvider execution target routing
- Kaggle/API routing through plugin/ComputeProvider adapters
- prompt preflight
- artifact provenance

Do not add generated video unless a real use case justifies cost/complexity.

## Phase 9 - DaVinci-ready Production Pack

- SRT
- FCPXML/interchange
- stable track layout
- markers
- predictable filenames
- artifact relinking
- asset source report

Success:
- importing into DaVinci starts close to the creative-editing stage

## Phase 10 - Studio Packs UX

Initial packs:

- Christian Cinematic
- Christian Stick Explainer (after stick plugin exists)
- Bible Illustrated
- Night Devotional
- Sleep Scripture

Features:

- inheritance/overrides
- basic/customize/advanced settings
- automation levels
- Review Center
- project Kanban board

Implementation slices:

- P0 — versioned portable Studio Pack contract, deterministic inheritance/override resolution and compatibility/portability validation
- P1 — capability-aware catalog plus initial packs that only become usable when their required plugin capabilities exist
- P2 — Basic / Customize / Advanced UX, automation controls and Review Center over canonical state
- P3 — project Kanban integration and final restart/resume/read-only/portability hardening

`Christian Stick Explainer` is capability-gated. Its pack definition must not imply that a Stick Figure plugin exists before Phase 11.

P1 catalog implementation notes:

- portable catalog storage remains a composition of Studio Pack definitions, not another plugin registry
- availability is evaluated from the canonical PluginRegistry plus ephemeral runtime readiness
- initial usable packs target only capabilities present in checked-in manifests
- `Christian Stick Explainer` is gated by `stick_figure_visual`; no generic generated-image fallback may satisfy that semantic requirement
- runtime health, credential readiness and machine-local installation state are not persisted as portable catalog truth

## Phase 11 - Stick Figure Visual Plugin

Use the existing SceneIntent contract.

- map concepts to characters/actions/objects
- minimal animation preset
- whiteboard-like preset later
- plugin-specific thumbnail style
- no changes to core workflow

This phase validates that the plugin architecture actually supports a radically different visual production style.

Phase 11 implementation notes:

- checked-in `stick-figure-reference` is a process-isolated Plugin API v1 visual provider
- exact capability is `stick_figure_visual`; it deliberately does not advertise `generated_still`
- the existing `visual.generate` request/ArtifactStore execution path accepts either `generated_still` or `stick_figure_visual` when `visual_generate` is also present
- SceneIntent is deterministically projected to plugin-local characters/actions/objects
- the first renderer is offline procedural SVG with `minimal-motion` and thumbnail composition presets
- full whiteboard-like rendering remains deferred
- no Project / WorkflowStep / Job / Attempt state-machine change is required

## Phase 12 - Asset Library Intelligence

Start simple:

- metadata
- tags
- usage history
- last-used
- duplicate/reuse detection

Later, if the asset library grows:

- semantic embeddings
- visual similarity
- smarter local-first retrieval

Do not add embeddings prematurely.

Phase 12 implementation notes:

- canonical `artifacts` + ArtifactStore remain the media source of truth
- SQLite migration adds tag/usage relations and an exact SHA-256 lookup index
- source reuse is derived from existing portable provenance (`source_provider` + `source_asset_id`) rather than a second source database
- usage history is idempotent and produces deterministic last-used / used-recently facts
- desktop Asset Library shows metadata, tags, usage and exact/source reuse summaries in writer and read-only modes
- tag mutations require the existing writable workspace guard
- semantic embeddings, vector storage and visual-similarity models remain deferred

## Later / optional

- additional stock providers
- additional TTS providers
- Premiere/Final Cut exporters
- richer quality plugins
- cloud ComputeProviders
- Plugin Manager install/update flows
- signed packages
- marketplace only if ecosystem demand exists
- Resolve scripting only if interchange is insufficient

## Explicitly deferred

Do not prioritize:

- final video renderer
- generic transcoding pipeline
- NVENC/NVDEC orchestration
- proxy generation
- local large AI models
- local diffusion by default
- complex video effects
- color grading
- audio mixing
- Kubernetes
- Redis
- RabbitMQ
- Kafka
- distributed microservice infrastructure

The system is initially for one creator, one Mac, temporary remote workers and a local editor. Keep it that simple.
