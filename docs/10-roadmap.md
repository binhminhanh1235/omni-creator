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

## Phase 13 - Additional Stock Providers

Expand stock coverage through the existing provider-neutral visual pipeline.

Priority order:

1. Pixabay
2. Unsplash
3. one commercial provider, selected between Storyblocks and Shutterstock after API access/license validation

Implementation slices:

- P0 — Pixabay VisualProvider for image + video search, preview-first selection, selected-asset download and portable provenance
- P1 — Unsplash VisualProvider for image search with required attribution, hotlink-preview behavior and selected-use download tracking
- P2 — commercial provider integration; evaluate Storyblocks first and Shutterstock as the alternative, with licensing contained inside the plugin boundary

Architecture rules:

- SceneIntent, VisualCandidate, Asset, Artifact and workflow contracts remain provider-neutral
- provider search results normalize to the existing VisualCandidate contract
- full media is resolved only after selection
- API credentials stay machine-local and secret values are never persisted in portable project state
- selected production media is copied into the granted job workspace before ArtifactStore promotion
- provider/source IDs, creator/source-page information and license/provenance facts remain portable metadata
- provider rate-limit, caching, attribution, download-tracking and licensing requirements must be enforced by each adapter
- do not add provider-specific ranking state; reuse the existing relevance, cliché and reuse scoring
- no scraping, stockpiling or mass-download workflow

P0 Pixabay implementation notes:

- checked-in `pixabay` plugin supports image + video preview-first search and selected-asset download
- Pixabay's 24-hour API response caching requirement uses the generic machine-local `provider-cache` permission
- search candidates expose preview URLs only; production download URLs are resolved after selection
- API key values remain outside portable settings and cache keys/files
- source provider/asset ID, creator, source page, attribution and Content License label flow through portable provenance

P1 Unsplash implementation notes:

- checked-in `unsplash` plugin is image-only and reuses the existing `VisualCandidate` / selected Asset boundary
- previews use the hotlinked `photo.urls.*` values returned by Unsplash
- candidate and selected provenance carry photographer + Unsplash attribution links with `utm_source=omnicreator&utm_medium=referral`
- selected use must successfully call the provider's `photo.links.download_location` tracking endpoint before photo bytes are accepted
- the tracking URL and full CDN URL are transport details and are not persisted into canonical selected-output metadata
- the access key stays machine-local through `UNSPLASH_ACCESS_KEY`; no provider-specific core state is added

P2 Storyblocks implementation notes:

- Storyblocks is selected over Shutterstock after license-compatibility review because OmniCreator's current single-creator local ArtifactStore -> DaVinci workflow requires the licensed creator to retain selected raw media locally
- the checked-in `storyblocks` plugin supports image + video preview-first search behind the same VisualCandidate boundary
- Storyblocks HMAC authentication is generated per request from machine-local public/private keys and never persisted
- canonical OmniCreator `project_id` is passed at the visual-operation payload root when the upstream API requires a project identifier; the provider's end-user identifier stays machine-local
- test API credentials may search/preview, but `visual.fetch_selected` refuses to produce a promotable selected output unless machine-local API mode is explicitly `production`
- selected production downloads are requested only after selection, copied into the granted job workspace, then verified/promoted by core
- full-size Storyblocks CDN URLs, HMACs, private keys and provider user identifiers are transport/configuration details and are not persisted into canonical output metadata
- no stockpiling, bulk pre-download or raw-file redistribution workflow is added

Success:

- a SceneIntent can search multiple stock sources without changing core workflow
- provider fallback remains capability-driven
- selected assets retain enough provenance to audit their source and license path
- free-provider integrations can be tested deterministically offline
- commercial-provider promotion cannot succeed before licensing succeeds

## Phase 14 - Plugin Manager Local Lifecycle

Promote the plugin packaging V2 work into a guarded local-only lifecycle before any marketplace or remote distribution.

Implementation slices:

- P0 — installed plugin inventory + lifecycle contract
- P1 — safe local install / uninstall
- P2 — update + compatibility
- P3 — desktop Plugin Manager UX

P0 architecture rules:

- the existing `PluginRegistry` remains the single canonical discovered-plugin registry
- built-in and machine-local user-installed roots are scanned together through the existing manifest validation path
- enabled/disabled is machine-local lifecycle state, not portable Project/Artifact/Studio Pack truth
- inventory is a projection over the canonical registry and exposes manifest identity/version/API/types/capabilities plus install source/trust
- disabled plugins remain discoverable inventory entries; runtime readiness marks them unavailable instead of pretending the capability was never installed
- accepted registry entries are API-compatible by construction; rejected/incompatible packages remain scan diagnostics until the later Needs Attention UX projects them
- installation directories, staging/rollback bookkeeping and update availability remain outside the portable Data Root
- Studio Pack definitions remain capability-oriented and availability stays derived from registry + machine-local runtime/lifecycle state

P1 safety rules:

- start with local folder/package input only
- inspect and validate the manifest before activation
- stage first, then atomically activate
- rollback failed activation
- uninstall must not mutate canonical Project/Artifact/Job state
- inspection must never run arbitrary install scripts

P2 adds local update detection, API/version compatibility gates, safe upgrade/rollback and capability-impact preview.

P3 exposes Installed / Disabled / Needs Attention, enable/disable, install/update/uninstall, health/readiness, capabilities and compatibility warnings. Read-only Data Root sessions remain safe because Plugin Manager installation state is machine-local.

Explicit non-goals for the initial Phase 14 slices:

- marketplace
- remote package registry
- billing
- arbitrary install scripts
- automatic background updates
- signed-package PKI before the local lifecycle is verified
- cloud plugin distribution

Success:

- a creator can understand what plugins are installed on this machine without duplicating PluginRegistry state
- moving the Data Root does not carry machine-specific installation state
- disabled/incompatible plugins produce deterministic setup diagnostics rather than corrupting portable projects
- future install/update UX has a rollback-safe local contract to build on

## Phase 15 - Creator Production Orchestration

Connect the verified Phase 0-14 capabilities into one canonical creator workflow from Studio Pack + creator input to a DaVinci-ready Production Pack.

This phase is selected ahead of marketplace/signing/remote-registry work because the current implementation already has the required capability primitives but does not yet orchestrate them into the MVP flow described by the product architecture.

Implementation slices:

- P0 — deterministic versioned creator workflow plan compiled from Project + resolved Studio Pack and materialized into the existing WorkflowStep dependency DAG
- P1 — creator topic/script input plus LLMGateway content and SceneIntent orchestration with canonical artifacts/hashes

P1 implementation boundary:

- creator input is a versioned portable `CreatorInputV1` with only TOPIC or SCRIPT text, never provider/account/model/session fields
- topic mode uses the existing LLMGateway creator content helper; script mode preserves creator text without an unnecessary rewrite call
- deterministic segmentation produces canonical `SegmentV1` identities before Scene Intelligence
- SceneIntent generation reuses the existing structured LLMGateway helper and preserves segment identity/narration exactly
- content and scene-plan outputs are canonical project artifacts committed through existing Job / Attempt / ArtifactStore state
- verified identical artifacts are reused without new provider calls; changed creator input invalidates downstream workflow state through the existing DAG
- LLMGateway configuration/credential absence is a retryable setup condition surfaced through Review Center, not provider state persisted in the Project
- P1 does not execute visual, voice, GPU or export stages
- P2 — visual discovery/routing/review/fallback orchestration through existing stock/generated/stick provider boundaries

P2 implementation boundary:

- resolved Studio Pack route order is authoritative: generated/stick-first routes do not perform speculative stock search
- stock-first routes use preview-only candidate discovery and existing deterministic ranking before `VisualRoutingDecisionV1`
- resolved Studio Pack `quality_thresholds.visual` supplies the stock acceptance threshold
- Assisted and Balanced stock routes stop at an explicit review/selection gate; Autopilot may select the deterministic recommendation
- Assisted generated/stick routes require generation approval; Balanced/Autopilot may advance when no blocking condition exists
- full stock download is forbidden until candidate selection resolves; generated/stick execution is forbidden until its approval gate resolves
- selected execution is per-scene canonical Job/Attempt work; aggregate `visual.prepare/project` succeeds only when all scene outputs are verified
- identical scene action/selection/routing hashes reuse verified artifacts without provider work
- stock artifacts must preserve the reviewed provider asset identity plus the canonical `visual_routing` provenance
- generated/stick targets reuse the existing Studio Pack capabilities `generated_still` and `stick_figure_visual`; no provider-specific fields are added to Project state
- P3 — voice/TTS + ComputeProvider orchestration through existing Job/Attempt/GPU readiness contracts

P3 implementation boundary:

- canonical `CreatorContentV1.segments` materialize deterministic per-segment `tts/<segment_id>` WorkflowStep + Job work under the existing `voice.prepare/project` semantic stage
- segment TTS input hashes reuse the existing normalization, pronunciation, voice/model version, voice-direction and settings fingerprint contract
- `content.prepare/project -> tts/<segment_id> -> voice.prepare/project` dependencies are stored only in the canonical DAG; `production.pack/project` remains downstream of aggregate voice + visual completion
- machine-local voice plugin, ComputeProvider, immutable voice/model version and resolved settings are execution-time inputs, never portable Project/StudioPack provider state
- preflight and lock requirements reuse `SegmentTtsPreparationV1`; unresolved approval/runtime/provider conditions remain explicit GPU NOT_READY reasons
- schedulable segment jobs compose the existing deterministic Voice Burst and GPU readiness scheduler; no P3-specific queue or worker database is introduced
- remote execution uses the existing voice-take dispatch path, preserving take history, timing sidecar output, ComputeAttemptRuntimeContext and retry policy
- restart and worker loss continue through the existing remote journal reconciliation path; RUNNING work is treated as in-flight rather than redispatched
- a changed voice/model/settings input invalidates the affected TTS step and canonical downstream voice/ProductionPack work by hash
- `voice.prepare/project` succeeds only after every segment has a selected, physically verified audio artifact and verified timing artifact

- P4 — ProductionPack assembly plus creator Start/Resume/Review/Export UX and end-to-end restart/read-only/portability hardening

P4 implementation boundary:

- creator projects created from a Studio Pack immediately materialize the canonical Phase 15 workflow DAG; the desktop does not maintain a parallel run-state object
- Start / Resume sends TOPIC or SCRIPT into the existing P1 content + SceneIntent Job/Attempt/Artifact orchestration and lets cache/retry semantics remain canonical
- project-board state is derived from both semantic WorkflowSteps and execution Jobs so completed early jobs cannot make an incomplete creator workflow appear READY_TO_EDIT
- ProductionPack assembly reads only physically verified selected canonical content, SceneIntent, per-scene visual, narration and voice-timing artifacts
- voice timing is the authoritative stable timeline clock for narration/visual duration and subtitle offsets
- the assembled ProductionPack is itself committed as a `production.pack/project` Job/Attempt/Artifact and may be loaded after restart or Data Root movement, including read-only sessions
- normal desktop flow never asks the creator to hand-author ProductionPack JSON; canonical JSON may be inspected read-only for diagnostics
- Resolve export reuses the existing Phase 9 `ProductionPackageExporterV1`; a successful `export.production-pack` Job is the terminal signal for the project-board DONE state
- Review Center and existing GPU/compute state remain the surfaces for blockers and retries; P4 adds no shadow scheduler, workflow DB or machine-specific portable state

P0 architecture rules:

- `CreatorWorkflowPlanV1` is a provider-neutral deterministic plan, not a second workflow engine
- the plan compiles only semantic stages: content preparation, scene planning, visual preparation, voice preparation and Production Pack assembly
- materialization uses the existing SQLite `steps` and `dependencies` tables through canonical `StateStore` APIs
- the first stage is READY and dependent stages remain NOT_READY until the existing DAG transition logic releases them
- re-materializing the same plan is idempotent
- conflicting existing input hashes are rejected instead of silently rewriting canonical workflow state
- the plan contains no provider endpoints, model IDs, credentials, secret values or machine-local paths
- P0 performs no network/provider execution

Success:

- creating the creator workflow no longer requires inventing an orchestration layer outside canonical Project/WorkflowStep state
- later slices can execute existing LLM, visual, voice, compute and export capabilities against stable semantic stages
- the final phase removes the normal-flow requirement to hand-author internal ProductionPack JSON

Explicit non-goals:

- plugin marketplace / remote registry / billing
- signed-package PKI
- final rendering or generic transcoding
- provider-specific portable orchestration state
- a second scheduler or workflow database

## Later / optional

- additional TTS providers
- Premiere/Final Cut exporters
- richer quality plugins
- cloud ComputeProviders
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
