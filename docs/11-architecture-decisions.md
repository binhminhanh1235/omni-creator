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
