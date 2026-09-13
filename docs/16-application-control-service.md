# Application Control Service v1

Phase 18 P0 introduces `omnicreator-application`, a transport-neutral application boundary shared by Desktop today and future CLI/MCP adapters.

Tracking: Phase 18 #99, P0 #100.

## Boundary

```text
Desktop
   \
CLI ----> Application Control Service ----> omnicreator-core
   /
MCP
```

P0 implements only the middle layer. CLI, MCP, OpenRouter/provider migration, agent packages and MCP Tasks remain later Phase 18 slices.

`omnicreator-core` remains the canonical owner of Project, WorkflowStep, dependency DAG, Job, Attempt, ArtifactStore, PluginRegistry contracts, ComputeProvider contracts, Studio Pack semantics, creator orchestration and manual/external/recovery contracts. The application service coordinates those APIs but does not introduce another workflow engine, scheduler or database.

## Session model

A writable `ApplicationControlService` can only be constructed from an acquired `WorkspaceSession`. That preserves the existing single-writer Data Root lease as the authority for mutations.

A read-only service is constructed from `Workspace` and opens SQLite through `StateStore::open_read_only`. Every application mutation also performs an explicit application-layer writable guard and returns the machine-readable `read_only` error code. Read-only behavior therefore does not depend on a Desktop UI convention.

## Versioned contracts

Application responses use schema `omnicreator.application-control`, version `1`. Typed operation names identify the operation independently of a human message.

Errors use schema `omnicreator.control-error`, version `1`, with stable categories:

- `read_only`
- `writer_conflict`
- `not_found`
- `invalid_input`
- `invalid_transition`
- `blocked`
- `capability_unavailable`
- `provider_unavailable`
- `artifact_invalid`
- `stale_input`
- `internal`

Future transports must switch on these codes rather than parse the error message.

## Shared control surface

The v1 service exposes canonical inspection and control for:

- workspace access/revision and project inventory;
- project status, board, WorkflowSteps, Jobs, Review Center projection and creator run coordinator;
- project create/rename/delete and Studio Pack binding using existing `StateStore` semantics;
- Phase 17 per-step automatic execution policy inspection and ON/OFF mutation;
- manual Content and ScenePlan ingestion/import;
- manual/Asset Library visual ingestion, stock selection, generated approval and external generated handoff;
- manual voice/timing ingestion and external voice handoff;
- Production Recovery inspection and repair actions;
- ProductionPack assembly/rebuild and Resolve/DaVinci export through the existing production exporter.

The service never marks a stage successful by itself. Success still comes only from the existing canonical core paths that create/use Jobs, Attempts, verified Artifacts and WorkflowStep transitions.

## Machine-local runtime bridge

Plugin and ComputeProvider runtime ownership does not move into `omnicreator-application`. `ApplicationRuntimeInspectorV1` is an injected boundary for adapters that already own machine-local runtime context. It returns only sanitized readiness/capability DTOs.

No Tauri type, CLI parser or MCP type appears in the application crate.

## Sanitization

Portable application response DTOs favor canonical IDs and logical URIs. Workspace snapshots do not expose the Data Root path. Plugin/compute inspection contracts have no credential, bearer token, provider-private request payload or arbitrary filesystem fields.

File paths are accepted only as explicit input to the same guarded manual/import/recovery functions that already copy, validate and promote files into ArtifactStore. They are not persisted into portable application response truth.

## Desktop adoption in P0

Phase 17 Desktop workflow policy inspection and ON/OFF commands now delegate through `ApplicationControlService`. This is the first production consumer proving that the boundary is not a dead abstraction while avoiding a risky one-PR rewrite of the full Desktop application module.

Additional Desktop commands can migrate incrementally as later slices need them. Their business semantics must remain in the shared application/core path rather than be copied into CLI or MCP.

## Deterministic acceptance

`crates/omnicreator-application/tests/control_service.rs` covers:

1. a provider-free full-manual creator flow from Project through manual Content, ScenePlan, visual, voice/timing, ProductionPack and Resolve/DaVinci export;
2. explicit application-layer rejection of mutation in a read-only session;
3. Phase 17 OFF meaning only `automatic execution disabled`, followed by successful canonical manual takeover;
4. sanitized serialized project snapshots with no Data Root absolute path, bearer token marker or API-key field.

Existing core and Desktop CI remains the regression authority for the Phase 15 automatic golden path, Phase 16 manual/external/recovery flows, Review Center, Phase 17 workflow controls, plugins/GPU and Data Root behavior.
