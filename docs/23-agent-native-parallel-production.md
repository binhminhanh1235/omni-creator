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
- P4 — Pexels/visual + OmniVoiceStudio parallel production recipes — #118 — DONE / VERIFIED
- P5 — fan-in QA/recovery + crash/reconnect/contention/portability E2E hardening — #119 — implementation complete; verification authority is #119/#113

Verified P4 baseline before P5:

- exact P4 branch head `9df5022f5fa5133c9d0f0c1c6e4967b4409c6636`
- exact-head tree `49ea2c4c36f281599d6ff87b71046ac0542cdafa`
- exact-head CI #618 / run `34953103640`: PASS
- guarded squash merge/current baseline `eea466ec1b4c8f564774c8aa2daa90f46f9ea0ed`
- post-merge CI #619 / run `34953558152`: PASS across Rust / Plugins / Desktop

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

The coordinator may receive worker outputs concurrently, but canonical mutation remains ordered and serialized through one writable Application Control Service session. Per-item outcomes stay truthful: one rejected or stale item does not fabricate failure or success for sibling items.

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
- return candidate result plus truthful provenance only.

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

The P4 package lives at:

- `agent-harness/parallel-production/contract.json`
- `agent-harness/parallel-production/README.md`
- `agent-harness/parallel-production/README.vi.md`

Executable verification lives in `crates/omnicreator-cli/tests/phase19_parallel_production_recipes.rs`.

## P5 — Fan-in QA and recovery

P5 adds a second read-only projection, `omnicreator.agent-fan-in-qa` v1. It does not replace `agent_work_graph`. The two views answer different questions:

- `agent_work_graph`: what canonical work is executable, running, blocked, reviewable or satisfied?
- `agent_fan_in_qa`: are the selected canonical visual/audio/timing artifacts physically healthy enough for final fan-in and recovery?

The projection combines only existing canonical sources:

1. `agent_work_graph` for per-unit work state;
2. Production Recovery for selected visual/audio/timing artifact verification at the active Data Root binding.

No worker-status database, harness claim table, retry scheduler or parallel-work persistence is introduced.

### P5 contract

`agent_fan_in_qa` returns:

- visual and voice units with canonical state;
- selected artifact IDs;
- matching Production Recovery items;
- per-unit recovery guidance;
- `ready_work_ids`;
- `needs_review_work_ids`;
- `unhealthy_canonical_units`;
- `recovery_ready_for_rebuild`;
- `fan_in_verified`;
- `production_pack_ready`;
- `production_pack_satisfied`.

A READY unit with no selected artifact is normal pending work and is not an artifact failure. A unit enters `unhealthy_canonical_units` only when its canonical work is already SATISFIED but its selected production artifact is missing, invalid or unexpectedly unselected.

Recovery guidance maps to existing typed paths only:

- `dispatch_external`: prepare current work and dispatch it;
- `review`: inspect Review Center/current graph before retry or replacement;
- `repair_visual`: use canonical production recovery/relink controls;
- `repair_voice_bundle`: repair audio plus timing through the existing canonical voice path;
- `assemble_production_pack`: only after the verified fan-in gate.

MCP exposes `agent_fan_in_qa`. CLI exposes `work qa --project <id>`. Both are inspection-only and do not acquire the writer lease.

### Reconnect and worker loss

After coordinator crash or reconnect:

1. discard harness-local claims/completion memory;
2. cold-read `agent_work_graph`;
3. cold-read `agent_fan_in_qa`;
4. reuse SATISFIED units whose selected artifacts still verify;
5. prepare only current READY work;
6. let `stale_input` reject obsolete worker output;
7. serialize new commits through the existing writer lease.

A verified visual committed before a coordinator restart remains selected and reusable after reconnect. P5 E2E covers this exact split by committing one lane, reopening read-only state, then completing the remaining lane.

### Writer contention

Read-only graph and QA inspection must remain available while another process holds the Data Root writer lease. A second mutating `work commit` must receive typed `writer_conflict`; it may not bypass the canonical lease or create a shadow writer.

### Data Root portability

After Data Root move/rebind, cold inspection is performed against the new root. Durable project/artifact references remain logical and path-independent. `agent_fan_in_qa` verifies artifacts at the current binding and must not serialize old/new absolute Data Root paths, credentials or bearer secrets.

### End-to-end fan-in gate

Before ProductionPack/Resolve export:

1. re-inspect `agent_work_graph`;
2. inspect `agent_fan_in_qa`;
3. require `fan_in_verified=true` and no unresolved unhealthy/review unit relevant to the production inputs;
4. repair/review through existing typed controls if required;
5. assemble or rebuild ProductionPack through the canonical production service;
6. regenerate Resolve/DaVinci interchange through the existing exporter;
7. cold-inspect final canonical state before reporting completion.

The E2E test `crates/omnicreator-application/tests/agent_fan_in_qa.rs` covers reconnect, artifact reuse, visual/voice fan-in, ProductionPack/Resolve export, Data Root move, path/secret sanitization and physical artifact loss after canonical success. CLI/MCP transport tests cover the real binary surface, and Phase 19 contention coverage keeps the single-writer rule explicit.

## Canonical writer rule

Parallel execution does not mean parallel mutation of the Data Root.

```text
worker A ---- result ----\
worker B ---- result -----+--> serialized canonical ingress --> OmniCreator
worker C ---- result ----/
```

No worker may edit SQLite, ArtifactStore files, workflow rows, selected-artifact fields, or completion flags directly.

## Safety and portability

Portable project truth must not contain:

- API keys, bearer tokens, cookies or credentials;
- provider-private sessions;
- worker/device-specific secrets;
- absolute Data Root paths;
- harness-specific scheduler state;
- rate-limit counters or machine-local concurrency settings.

Read-only Workspace sessions may inspect graph and fan-in QA state but cannot mutate canonical state.

## Target harness workflow

1. inspect project and `agent_work_graph`;
2. satisfy or generate Content;
3. delegate READY voice units while ScenePlan is produced;
4. after ScenePlan commits, delegate READY visual units;
5. collect worker outputs outside canonical storage;
6. serialize accepted outputs through `agent_work_commit`;
7. re-inspect graph plus `agent_fan_in_qa` after each commit wave;
8. on reconnect, rebuild pending work only from current canonical projections;
9. route `NEEDS_REVIEW` or unhealthy satisfied units to typed Review Center/production recovery;
10. assemble ProductionPack only after verified canonical visual plus voice fan-in;
11. verify ProductionPack/Resolve output from canonical state before reporting completion.

The harness remains replaceable. A project started with Claude Code can be resumed through Codex, Antigravity, Desktop, CLI, or another MCP client because durable truth remains OmniCreator state.

## Status

P0 through P4 are DONE / VERIFIED. P5 #119 completes the implementation described here. Exact verification status, branch SHA/tree, exact-head CI, guarded merge and post-merge CI evidence are authoritative in #119 and umbrella #113. P5 is DONE / VERIFIED only when those gates are all PASS.
