# Phase 19 — Agent-Native Parallel Production

Tracking: umbrella #113, P0 #114.

## Goal

Phase 19 makes OmniCreator a production engine that agent harnesses can coordinate in parallel without turning Codex, Claude Code, Google Antigravity, or any worker pool into a second workflow database.

The boundary is deliberately asymmetric:

```text
Codex / Claude Code / Antigravity
       reason / delegate / fan out
                  |
              MCP / CLI
                  |
      Application Control Service
                  |
       canonical OmniCreator state
  Project / WorkflowStep / Job / Attempt / Artifact
          /                    \
  visual workers            voice workers
          \                    /
             verified fan-in
                  |
         ProductionPack / Resolve
```

Harnesses own creative reasoning, delegation, and process-level concurrency. OmniCreator owns deterministic work identity, dependency truth, artifact verification, recovery, and final production state.

## Phase 19 slices

- P0 — canonical agent work graph / typed parallel-work descriptors — #114
- P1 — batch external result ingress + serialized canonical commit — #115
- P2 — CLI + MCP parallel-work control surface — #116
- P3 — harness worker-pool packages for Codex, Claude Code and Antigravity — #117
- P4 — Pexels/visual + OmniVoiceStudio parallel production recipes — #118
- P5 — fan-in QA/recovery + crash/reconnect/portability E2E hardening — #119

## P0 — Agent work graph

P0 adds a read-only projection in `omnicreator-application`.

It does not create a scheduler, queue, lease table, claim table, or agent database. Every work item is derived from existing canonical state each time it is inspected.

The versioned contract is `omnicreator.agent-work-graph` v1.

### Work kinds

- `content`
- `scene_plan`
- `visual` per canonical scene
- `voice` per canonical segment
- `production_pack`

### Work states

- `BLOCKED` — canonical dependencies are not satisfied
- `READY` — work can be executed or satisfied through an existing canonical/manual/external path
- `RUNNING` — canonical Job state says work is queued/running
- `NEEDS_REVIEW` — canonical failed/retryable/fatal/stale/cancelled state needs recovery
- `SATISFIED` — verified canonical state/artifacts satisfy the unit

### Parallelism semantics

Voice depends on canonical Content, not ScenePlan. Therefore voice workers can start as soon as Content is verified while a storyboard/ScenePlan worker continues independently.

Visual units depend on canonical ScenePlan. Once ScenePlan is verified, individual visual scene units become independently discoverable and can fan out to workers.

```text
                    Content
                    /     \
                   /       \
             ScenePlan     Voice S01
             /  |  \       Voice S02
            /   |   \      Voice S03
      Visual01 Visual02 Visual03
            \    |     /      /
             \   |    /      /
              verified fan-in
                    |
             ProductionPack
```

ProductionPack remains blocked until canonical visual and voice aggregate stages are complete. The work graph cannot mark fan-in complete merely because an external worker reported success.

### External descriptors

P0 reuses the verified external-handoff contracts already owned by core:

- visual: `ExternalGeneratedVisualRequestV1`
- voice: `ExternalVoiceRequestV1`

Each descriptor carries a deterministic `request_sha256`. Existing result-ingress code recomputes the current request and rejects an obsolete result when Content or ScenePlan changed while a worker was running.

P0 intentionally does not pretend the visual descriptor is a generic stock-provider request. Pexels/other stock capability-aware dispatch belongs to P4, where provider-specific execution guidance can be layered over canonical scene work without leaking provider state into portable Project truth.

## Canonical writer rule

Parallel execution does not mean parallel mutation of the Data Root.

Workers may execute concurrently outside OmniCreator, but canonical result commits go through the shared Application Control Service under the existing single-writer WorkspaceSession lease.

```text
worker A ---- result ----\
worker B ---- result -----+--> serialized canonical ingress --> OmniCreator
worker C ---- result ----/
```

No worker may edit SQLite, ArtifactStore files, workflow rows, or selected-artifact fields directly.

## Idempotency and stale work

Phase 19 builds on existing Job/Attempt/input-hash and external request-hash semantics rather than inventing agent-specific identity.

P0 exposes input identity. P1 adds batch serialized ingress and explicit duplicate/reconnect behavior. A later result must be rejected when its prepared request no longer matches current Content/ScenePlan.

## Safety and portability

The work graph contains canonical IDs, logical state and sanitized external descriptors. It must not serialize:

- API keys, bearer tokens, cookies, credentials
- provider-private sessions
- worker/device-specific secrets
- absolute Data Root paths
- a harness-specific scheduler identity

Read-only Workspace sessions may inspect the exact same work graph but cannot mutate canonical state.

## Target harness workflow

A coordinator harness should:

1. inspect project/work graph;
2. satisfy or generate Content;
3. re-inspect;
4. delegate ScenePlan and all READY voice units concurrently;
5. after ScenePlan is committed, re-inspect and delegate READY visual units;
6. collect worker outputs outside canonical storage;
7. submit outputs through typed OmniCreator result-ingress operations;
8. re-inspect after each mutation/batch;
9. route `NEEDS_REVIEW` units to Review Center/recovery;
10. assemble ProductionPack only after verified fan-in.

The harness remains replaceable. A project started with Claude Code can be resumed through Codex, Antigravity, Desktop, CLI, or another MCP client because the durable truth is still OmniCreator state.

## P0 implementation files

- `crates/omnicreator-application/src/agent_work.rs`
- `crates/omnicreator-application/tests/agent_work_graph.rs`

The acceptance test proves Content -> Voice fan-out, ScenePlan -> Visual fan-out, verified per-unit fan-in, ProductionPack readiness, read-only parity, and serialization sanitization.

## Status

P0 implementation is in progress on `feat/phase19-p0-agent-work-graph`. Exact-head CI and post-merge verification remain required before P0 can be marked DONE / VERIFIED.
