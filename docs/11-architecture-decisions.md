# Architecture Decisions and Non-goals

This document records decisions so later development does not accidentally reintroduce discarded complexity.

## ADR-001: Local-first control plane

**Decision:** Project truth lives locally on the Mac.

**Reason:** Kaggle sessions are temporary; DaVinci and media editing are local; resume/retry requires durable state.

## ADR-002: Kaggle is a ComputeProvider, not the application host

**Decision:** Kaggle runs GPU jobs only.

**Reason:** Limited free GPU time is more valuable when used for inference rather than planning, downloads, waiting or project state.

## ADR-003: Burst compute

**Decision:** Prepare multiple projects to GPU READY before starting a Kaggle session.

**Reason:** Maximizes useful GPU work per session and reduces idle/setup overhead.

## ADR-004: Rust core + Python AI workers

**Decision:** Rust/Tauri for product/control plane; Python for model-specific AI.

**Reason:** Rust is excellent for orchestration, async I/O, local app reliability and small footprint. Python remains the strongest AI ecosystem.

## ADR-005: Process-isolated plugin system

**Decision:** Plugins communicate through a versioned JSON protocol instead of native Rust dynamic libraries.

**Reason:** Language neutrality, crash isolation, easier AI plugin development and simpler remote adaptation.

## ADR-006: SceneIntent is the visual abstraction

**Decision:** Visual providers receive semantic SceneIntent rather than provider-specific input.

**Reason:** Pexels, stick figure, illustration and future providers can replace one another without changing the core content pipeline.

## ADR-007: Studio Packs are the default UX

**Decision:** Normal users choose a Studio Pack. Plugin wiring is Advanced.

**Reason:** Plugin architecture should create flexibility for the system without creating complexity for the user.

## ADR-008: Content relevance first

**Decision:** Candidate ranking heavily weights semantic, emotional and narrative relevance.

**Reason:** The primary product goal is better content, not technical media optimization.

## ADR-009: Pexels preview-first

**Decision:** Rank preview frames/metadata before downloading full stock assets.

**Reason:** Reduces bandwidth, storage and pointless downloads while preserving candidate quality.

## ADR-010: DaVinci owns video finishing

**Decision:** OmniCreator exports production assets/timeline instead of implementing a full editor.

**Reason:** DaVinci already solves editing, grading, effects, audio mix, proxies and rendering.

## ADR-011: No default encode/transcode pipeline

**Decision:** Do not add NVENC/NVDEC management, generic encoding or proxy rendering to OmniCreator MVP.

**Reason:** Stock assets can be downloaded directly. Any editing-format optimization should be handled by DaVinci unless a concrete future bottleneck proves otherwise.

## ADR-012: Incremental DAG + hashes

**Decision:** Every expensive operation is tracked as a dependency-aware job with input hashes and artifacts.

**Reason:** Resume, retry, cache reuse and small edits must not trigger entire-project regeneration.

## ADR-013: Immediate remote artifact sync

**Decision:** Transfer and verify completed Kaggle artifacts continuously.

**Reason:** A lost session should lose at most the currently unfinished job, not hours of completed work.

## ADR-014: SQLite, not distributed infrastructure

**Decision:** Use SQLite and local filesystem for v1.

**Reason:** Initial topology is one user, one local app and temporary remote workers. Kafka/Redis/RabbitMQ/Kubernetes would add complexity without value.

## ADR-015: No local heavy AI by default

**Decision:** M2 Pro local compute is reserved for the app and DaVinci workflow. Heavy AI goes to Kaggle/API.

**Reason:** 16 GB unified memory is useful for editing. Running large models locally competes with the creator workflow without clear benefit.

## ADR-016: Adaptive media resolution

**Decision:** Technical resolution is chosen after content selection and based on edit need.

**Reason:** 4K should be used when crop/reframe/hero quality benefits, not by default.

## ADR-017: Asset provenance is first-class

**Decision:** Store source, license/provenance, creator/source URL, hashes and usage metadata.

**Reason:** Makes copyright/source audits possible and avoids losing origin information.

## ADR-018: Multiple takes are preserved

**Decision:** Regeneration creates new takes/attempts instead of destructive overwrite.

**Reason:** Supports A/B selection, retry history and safe rollback.

## ADR-019: User-selected portable Data Root

**Decision:** All durable creator/project data lives under one user-selected Data Root.

**Reason:** The user must be able to move, copy or synchronize the workspace and continue on another machine.

## ADR-020: No durable absolute paths

**Decision:** Canonical state uses logical URIs/artifact IDs; absolute paths are resolved only at machine I/O/export boundaries.

**Reason:** User home paths, Google Drive mount paths and OS path conventions differ between machines.

## ADR-021: Cloud sync is device handoff, not concurrent collaboration

**Decision:** A synchronized Data Root supports one active writer at a time.

**Reason:** Google Drive-style file synchronization is not a distributed transaction/locking system and SQLite should not be treated as a multi-machine live database through a sync folder.

## ADR-022: Clean handoff snapshots

**Decision:** Graceful close/device handoff creates a verified clean state snapshot and handoff manifest.

**Reason:** A synced/copyable recovery point protects against partial synchronization or interrupted copies.

## ADR-023: Secrets are machine-local

**Decision:** Plaintext API keys/passwords are not stored in the portable Data Root by default.

**Reason:** A workspace may be synced through third-party cloud storage. Secrets belong in the OS credential store; the workspace stores symbolic credential references.

## ADR-024: Portable data is not portable runtime

**Decision:** Projects, assets, state, profiles, Studio Packs and plugin configuration are portable. Disposable caches, platform-specific plugin binaries and AI runtimes remain machine-local.

**Reason:** Copying runtime/cache bloat wastes sync/storage and can be incompatible across machines. The new machine should resolve/install required runtimes from a plugin lock/config.

## ADR-025: Provider-neutral HTTP compute bridge

**Decision:** The desktop may connect disposable GPU workers through a small versioned HTTP adapter implementing the existing ComputeProvider and ComputeProviderExecution contracts.

**Reason:** The Workbench needs a real connect/dispatch/journal/artifact path without putting Kaggle-specific fields in core. Endpoint configuration is machine-local, credentials are referenced by environment-variable name, and SQLite remains canonical.

## ADR-026: Studio Packs compose capabilities, not provider internals

**Decision:** Portable Studio Pack contracts select semantic routes, plugin capability/type requirements, optional plugin IDs, preset IDs, automation level and quality policy. Provider endpoints, model-specific request fields, plaintext credentials and machine paths remain outside the Studio Pack core contract.

**Reason:** Studio Packs are the user-facing composition layer, not a second plugin registry or workflow engine. Keeping their durable contract capability-oriented preserves plugin replaceability, Data Root portability and existing ownership boundaries.

Inheritance uses one parent and deterministic replace/remove overrides. Missing optional v1 fields use defaults, while unknown v1 fields and unsupported versions are rejected explicitly so arbitrary provider/secret blobs cannot silently enter portable state.

Pack availability is resolved against the real PluginRegistry/capability state. A definition such as Christian Stick Explainer stays unavailable until its required stick-figure capability/plugin exists.

## Non-goals for MVP

OmniCreator is not:

- a replacement for DaVinci Resolve
- a final-render engine
- a distributed SaaS backend
- a cloud storage system
- a GPU benchmark tool
- a plugin marketplace
- a general-purpose media transcoder
- a local large-model workstation

If a future feature proposal conflicts with these decisions, require an explicit new ADR with evidence that the tradeoff has changed.


## ADR-027: Studio Pack availability is derived runtime state

**Decision:** Portable Studio Pack catalog data stores only versioned pack definitions. Availability is evaluated from the resolved Studio Pack against the canonical PluginRegistry and ephemeral machine-local runtime readiness facts.

**Reason:** Plugin installation, health and credential readiness can differ between machines that share the same portable Data Root. Persisting those facts would turn a transient setup condition into false project truth and would duplicate PluginRegistry ownership.

The P1 evaluator exposes `AVAILABLE`, `AVAILABLE_WITH_SETUP` and `UNAVAILABLE` with machine-readable reasons. Missing preferred or fallback implementations are non-corrupting diagnostics when another compatible target is usable.

`Christian Stick Explainer` uses `stick_figure_visual` as its semantic capability gate. The definition may ship before Phase 11, but it remains unavailable until a compatible visual plugin advertises that exact capability. Generic generated still/image capability does not satisfy the stick-figure semantic contract.

## ADR-028: Phase 10 P2 UI is a projection over canonical state

**Decision:** Basic / Customize / Advanced Studio Pack UX and Review Center are projections/controllers over existing Studio Pack, PluginRegistry, Project, WorkflowStep, Job, Attempt, LLMGateway, ComputeProvider and ProductionPack ownership boundaries.

**Reason:** Phase 10 exists to reduce creator complexity, not to introduce a second workflow engine or UI database.

Project-specific creative customization is an ordinary child `StudioPackV1` in the portable catalog and is resolved by the existing deterministic inheritance/override resolver. `Project.studio_pack` remains the project binding. Resetting customization rebinds the project to its built-in parent.

Review Center is rebuilt from canonical state on demand. Retry actions use canonical Job transitions. Automation levels are deterministic policy projections and never become shadow Job/Attempt state.

Machine-local plugin credential readiness is evaluated ephemerally from symbolic environment-variable references; plaintext credential values and machine-specific absolute paths remain outside portable Studio Pack/project data. `Christian Stick Explainer` remains gated by the exact `stick_figure_visual` capability.
\n## ADR-029: Project Kanban is derived, never authoritative

**Decision:** The Phase 10 P3 project board is a projection of canonical `ProjectDisplayStatus`, WorkflowStep and Job state. Column placement is not directly editable and is not stored in browser state, SQLite columns or a second board database.

`GPU_PARTIAL` maps to the `NEEDS REVIEW` creator column because retryable partial GPU work requires action rather than representing a distinct lifecycle stage.

Action summaries are derived from canonical counts. Restart reconciliation, Data Root moves and read-only opens therefore reconstruct the same board from the portable workspace. Studio Pack, Review Center, GPU Workbench and Production Pack remain the owning surfaces for their respective mutations.

## ADR-030: Stick figure visuals reuse the existing visual.generate boundary

**Decision:** Phase 11 implements stick figures as a normal process-isolated visual plugin consuming the existing SceneIntent-backed generated-image request and `visual.generate` operation.

The plugin advertises exact semantic capability `stick_figure_visual`, `visual_generate` and deterministic-seed support. It deliberately does not advertise `generated_still`, preventing generic generated-image routes from selecting it accidentally. The existing visual-generation preflight accepts either `generated_still` or `stick_figure_visual` as a semantic generation capability while still requiring `visual_generate`.

Characters, actions and objects are renderer-local deterministic projections of SceneIntent. They are returned as artifact metadata, not stored as new core workflow state. The reference renderer writes offline SVG only inside the granted job workspace and returns portable relative output plus provenance.

**Reason:** This validates ADR-006 in practice: a radically different visual style can plug into the frozen SceneIntent/Plugin API contracts without a second workflow engine, new provider-specific project fields or changes to Job/Attempt semantics.

## ADR-031: Asset Library intelligence indexes canonical artifacts

**Decision:** Phase 12 keeps `ArtifactStore` + SQLite `artifacts` canonical and adds relational tags/usages plus derived exact-hash/source-reuse projections. It does not create a second asset/blob database.

Exact duplicate detection uses the SHA-256 already required on every artifact. Source reuse uses existing portable provenance identifiers when available. Last-used and used-recently are derived from idempotent usage rows. `AssetV1` is not expanded with workflow-specific usage state.

Asset Library UI is a derived view. Writer sessions may mutate tags through the canonical StateStore; read-only sessions may inspect the same projection but cannot mutate it. Moving the Data Root preserves the projection because durable state contains artifact IDs, project/context IDs and logical URIs rather than machine paths.

**Reason:** This provides useful reuse intelligence immediately without premature embeddings, vector databases, background indexing services or provider-specific project state. More expensive semantic/visual similarity can be added later only if library scale justifies it.


## ADR-032: Stock provider policy stays inside VisualProvider adapters

**Decision:** Phase 13 adds Pixabay, Unsplash and a later access-gated commercial stock provider behind the existing SceneIntent -> VisualCandidate -> selected Asset boundary. Core does not gain provider-specific search, attribution, licensing or ranking state.

Provider search returns preview-safe `VisualCandidate` records plus opaque `selection_ref` values. Full-resolution/download URLs are resolved only after selection and are not persisted as canonical project truth. Selected production media is written into the granted job workspace and promoted only after core verification.

A generic machine-local `provider-cache` permission may be granted when an upstream API requires response caching. Cache state is scoped by plugin and is never portable or canonical. Credentials remain environment-backed machine-local configuration.

**Reason:** Stock APIs differ mainly in transport policy, attribution, caching, rate limits and licensing. Keeping those differences inside adapters lets additional providers participate in the existing relevance, cliche and reuse scoring without fragmenting SceneIntent, workflow state or ArtifactStore ownership.
