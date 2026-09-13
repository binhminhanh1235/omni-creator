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

A writable `ApplicationControlService` or `CreatorRunControlServiceV1` can only be constructed from an acquired `WorkspaceSession`. That preserves the existing single-writer Data Root lease as the authority for mutations.

A read-only service is constructed from `Workspace` and opens SQLite through `StateStore::open_read_only`. Every application mutation also performs an explicit application-layer writable guard and returns the machine-readable `read_only` error code. Read-only behavior therefore does not depend on a Desktop UI convention.

## Versioned contracts

Application responses use versioned serde contracts. General control responses use schema `omnicreator.application-control`, version `1`; shared creator Start/Resume uses schema `omnicreator.creator-run-control`, version `1`, and the typed `start_or_resume` operation. Operation identity is independent of human messages.

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

The v1 application boundary exposes canonical inspection and control for:

- workspace access/revision and project inventory;
- project status, board, WorkflowSteps, Jobs, Review Center projection and creator run coordinator;
- project create/rename/delete and Studio Pack binding using existing `StateStore` semantics;
- Phase 17 per-step automatic execution policy inspection and ON/OFF mutation;
- canonical creator Start/Resume sequencing across Content, Scene Plan, visual, voice/compute and ProductionPack;
- manual Content and ScenePlan ingestion/import;
- manual/Asset Library visual ingestion, stock selection, generated approval and external generated handoff;
- manual voice/timing ingestion and external voice handoff;
- Production Recovery inspection and repair actions;
- ProductionPack assembly/rebuild and Resolve/DaVinci export through the existing production exporter.

The application boundary never fabricates success. Start/Resume checks the Phase 17 automatic-execution policy before invoking a stage. OFF means the automatic executor is not invoked and the canonical result remains required. Stage completion still comes only from existing core paths that create/use Jobs, Attempts, verified Artifacts and WorkflowStep transitions.

## Machine-local runtime bridge

Plugin and ComputeProvider runtime ownership does not move into `omnicreator-application`. `ApplicationRuntimeInspectorV1` remains the sanitized inspection boundary.

Creator Start/Resume additionally uses the narrow `CreatorRunRuntimeV1` execution bridge. That trait supplies machine-local LLM/plugin/compute execution to the shared controller, while cross-stage sequencing, policy gates and ProductionPack assembly stay in `omnicreator-application`. Runtime implementations must call the existing canonical core stage APIs; they are not a second workflow engine.

No Tauri type, CLI parser or MCP type appears in the application crate.

## Sanitization

Portable application response DTOs favor canonical IDs and logical URIs. Workspace snapshots do not expose the Data Root path. Plugin/compute inspection contracts have no credential, bearer token, provider-private request payload or arbitrary filesystem fields.

File paths are accepted only as explicit input to the same guarded manual/import/recovery functions that already copy, validate and promote files into ArtifactStore. They are not persisted into portable application response truth.

## Desktop adoption in P0

Phase 17 Desktop workflow policy inspection and ON/OFF commands delegate through `ApplicationControlService`.

The Phase 17 `start_creator_production_phase17` command is also a thin adapter over `CreatorRunControlServiceV1::start_or_resume_v1`. Desktop supplies `DesktopCreatorRunRuntimeV1` only for machine-local LLM/plugin/ComputeProvider execution. The Tauri command no longer owns the Content -> Scene Plan -> visual -> voice -> ProductionPack sequence or the AUTO ON/OFF gates.

This proves the application boundary is a production consumer rather than a dead abstraction and prevents future CLI/MCP adapters from copying the former Tauri orchestration.

## Deterministic acceptance

`crates/omnicreator-application/tests/control_service.rs` covers:

1. a provider-free full-manual creator flow from Project through manual Content, ScenePlan, visual, voice/timing, ProductionPack and Resolve/DaVinci export;
2. explicit application-layer rejection of mutation in a read-only session;
3. Phase 17 OFF meaning only `automatic execution disabled`, followed by successful canonical manual takeover;
4. sanitized serialized project snapshots with no Data Root absolute path, bearer token marker or API-key field.

`crates/omnicreator-application/tests/creator_run_control.rs` additionally covers:

1. shared Start/Resume refusing to invoke provider execution when `content.prepare` AUTO is OFF;
2. provider-free manual artifacts being resumed through the shared controller to canonical ProductionPack assembly;
3. read-only Start/Resume rejection before machine-local runtime resolution;
4. serialized Start/Resume output remaining free of Data Root paths and credential markers.

Existing core and Desktop CI remains the regression authority for the Phase 15 automatic golden path, Phase 16 manual/external/recovery flows, Review Center, Phase 17 workflow controls, plugins/GPU and Data Root behavior.
