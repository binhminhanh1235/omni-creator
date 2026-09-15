# Phase 19 — Agent-Native Parallel Production

Tracking: umbrella #113, P0 #114, P1 #115, P2 #116, P3 #117, P4 #118, P5 #119.

## Goal

Phase 19 makes OmniCreator a production engine that agent harnesses can coordinate in parallel without turning Codex, Claude Code, Google Antigravity, or any worker pool into a second workflow database.

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

Harnesses own creative reasoning, delegation, provider-local concurrency and external process execution. OmniCreator owns deterministic work identity, dependency truth, serialized canonical mutation, artifact verification, recovery, and final production state.

## Phase 19 slices

- P0 — canonical agent work graph / typed parallel-work descriptors — #114 — DONE / VERIFIED
- P1 — batch external result ingress + serialized canonical commit — #115 — DONE / VERIFIED
- P2 — CLI + MCP parallel-work control surface — #116 — DONE / VERIFIED
- P3 — harness worker-pool packages for Codex, Claude Code and Antigravity — #117 — DONE / VERIFIED
- P4 — Pexels/visual + OmniVoiceStudio parallel production recipes — #118 — IN PROGRESS
- P5 — fan-in QA/recovery + crash/reconnect/portability E2E hardening — #119 — NOT STARTED

## P0 — Agent work graph

P0 adds a read-only projection in `omnicreator-application`. It does not create a scheduler, queue, lease table, claim table, or agent database. Every work item is derived from existing canonical state each time it is inspected.

The versioned contract is `omnicreator.agent-work-graph` v1.

Work kinds:
- `content`
- `scene_plan`
- `visual` per canonical scene
- `voice` per canonical segment
- `production_pack`

Work states:
- `BLOCKED`
- `READY`
- `RUNNING`
- `NEEDS_REVIEW`
- `SATISFIED`

Voice depends on canonical Content, not ScenePlan. Visual units depend on canonical ScenePlan. ProductionPack remains blocked until canonical visual and voice aggregate stages are complete.

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

The work graph exposes deterministic input identity and typed external descriptors. Visual external descriptors are generated-still descriptors, not generic stock requests. Voice external descriptors carry the canonical narration/timing contract.

## P1 — Serialized external result ingress

P1 adds typed batch result ingress for generated visual and voice external workers.

The coordinator may receive worker outputs concurrently, but canonical mutation remains ordered and serialized through one writable Application Control Service session. Per-item outcomes stay truthful: one rejected/stale item does not fabricate failure or success for sibling items.

The batch layer reuses core stale validation based on `request_sha256`, preserves explicit replacement semantics, and relies on canonical result hashing/cache behavior for replay-safe duplicate delivery. It does not add an agent-specific delivery database.

## P2 — CLI + MCP parallel-work surface

P2 exposes Phase 19 work controls through both typed transports:

- `agent_work_graph`
- `agent_work_prepare`
- `agent_work_commit`

The CLI exposes equivalent `work graph`, `work prepare`, and `work commit` flows. Complex mutations use typed JSON input rather than ad-hoc shell flags.

These transports do not own workflow truth. They are façades over the same Application Control Service and Data Root writer lease.

## P3 — Harness worker-pool package

P3 adds a shared worker-pool contract plus harness recipes for Codex, Claude Code and Google Antigravity.

Coordinator responsibilities:
- inspect `agent_work_graph`;
- prepare current work immediately before dispatch;
- fan out only independent READY work;
- collect external candidate outputs;
- serialize canonical commit batches;
- re-inspect after every mutation;
- rebuild the queue from canonical state after reconnect.

Worker responsibilities:
- execute only the provided external task;
- never call `agent_work_commit`;
- never edit SQLite, ArtifactStore, Data Root internals, selected artifacts or completion flags;
- return candidate result + truthful provenance only.

Concurrency is harness-local policy, not persisted workflow state.

Reference package:
- `agent-harness/worker-pool/contract.json`
- `agent-harness/worker-pool/README.md`
- `agent-harness/{codex,claude,antigravity}/WORKER-POOL.md`

## P4 — Parallel production recipes

P4 makes the most common real production split explicit: stock/generated/manual visual workers per scene and OmniVoiceStudio/manual voice workers per segment.

### Visual lane separation

Stock, generated and manual visual paths are intentionally distinct.

For Pexels stock work:
1. canonical Studio Pack intent and available runtime capability must allow stock;
2. use `visual.resolve` for preview-first metadata discovery;
3. inspect candidate metadata/previews and select one candidate;
4. only then call `visual.fetch_selected` for the selected full media;
5. commit the chosen image/video through a canonical visual path.

Pexels advertises `stock_video`, `stock_image`, `preview_first_search`, and `selected_asset_download`. The recipe forbids speculative full-media downloads for all search results.

The Phase 19 external visual descriptor is the generated-still contract. It accepts supported still images and must not be used to submit Pexels stock video. Externally fetched stock image/video fallback can instead use the canonical manual visual path with truthful `EXTERNAL_RESULT_IMPORT` provenance.

Generated still work uses the current prepared `visual.generate` descriptor and keeps its `request_sha256`. Manual visual takeover remains valid whenever stock/generated capability is unavailable or a human-selected asset is preferred.

### OmniVoiceStudio lane

For every READY voice unit:
1. coordinator calls `agent_work_prepare`;
2. external worker receives the canonical `tts.generate` descriptor;
3. OmniVoiceStudio renders WAV/MP3 plus required timing for the same segment;
4. worker returns paths/result metadata only;
5. coordinator commits with source label `omnivoice-studio`;
6. coordinator immediately re-inspects `agent_work_graph`.

If Content changes while rendering, the old request hash is stale and must be rejected. If OmniVoiceStudio/compute is unavailable, a canonical manual voice bundle with timing is the supported fallback.

### Runtime boundary

Current MCP execution intentionally does not pretend to own machine-local visual plugin or voice/compute runtime. MCP can coordinate canonical external/manual handoff while Desktop or another machine-local runtime performs plugin/compute execution.

Provider concurrency, retry state, credentials, machine paths, device IDs, Pexels rate limits and OmniVoiceStudio/GPU capacity stay machine-local or harness-local. They are never portable Project truth.

The P4 machine-readable package lives at:
- `agent-harness/parallel-production/contract.json`
- `agent-harness/parallel-production/README.md`
- `agent-harness/parallel-production/README.vi.md`

Executable verification lives in `crates/omnicreator-cli/tests/phase19_parallel_production_recipes.rs`. It checks contract drift, Pexels manifest capability alignment, skill parity, secret-like markers, and the real MCP tool surface.

## Canonical writer rule

Parallel execution does not mean parallel mutation of the Data Root.

```text
worker A ---- result ----\
worker B ---- result -----+--> serialized canonical ingress --> OmniCreator
worker C ---- result ----/
```

No worker may edit SQLite, ArtifactStore files, workflow rows, selected-artifact fields, or completion flags directly.

## Idempotency, stale work and reconnect

Phase 19 builds on canonical Job/Attempt/input-hash and external request-hash semantics rather than inventing agent-specific identity.

- stale result: reject and prepare current work again;
- duplicate/reconnect delivery: use canonical ingress/idempotency behavior;
- writer conflict: do not open another writer, back off and retry through the coordinator;
- reconnect: reconstruct work solely from current canonical graph/state.

## Safety and portability

Portable project truth must not contain:
- API keys, bearer tokens, cookies or credentials;
- provider-private sessions;
- worker/device-specific secrets;
- absolute Data Root paths;
- harness-specific scheduler state;
- rate-limit counters or machine-local concurrency settings.

Read-only Workspace sessions may inspect the work graph but cannot mutate canonical state.

## Target harness workflow

1. inspect project/work graph;
2. satisfy or generate Content;
3. re-inspect;
4. delegate ScenePlan and all READY voice units concurrently;
5. after ScenePlan is committed, re-inspect and delegate READY visual units;
6. collect worker outputs outside canonical storage;
7. submit accepted outputs through typed OmniCreator result ingress;
8. re-inspect after each mutation/batch;
9. route `NEEDS_REVIEW` units to Review Center/recovery;
10. assemble ProductionPack only after verified canonical visual + voice fan-in.

The harness remains replaceable. A project started with Claude Code can be resumed through Codex, Antigravity, Desktop, CLI, or another MCP client because durable truth remains OmniCreator state.

## Status

P0–P3 are DONE / VERIFIED. P4 #118 is being implemented on `feat/phase19-p4-parallel-production-recipes`. It must not be marked DONE / VERIFIED until exact-head CI passes, guarded merge completes, merge tree is checked, and post-merge CI on `main` passes.
