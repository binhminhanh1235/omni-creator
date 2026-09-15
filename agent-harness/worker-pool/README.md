# OmniCreator harness worker pool

Tracking: Phase 19 P3 #117.

This package is a coordination recipe for Codex, Claude Code, Google Antigravity, and similar harnesses. It is not a scheduler inside OmniCreator. The harness may run external workers concurrently, while OmniCreator remains the only owner of canonical Project / WorkflowStep / Job / Attempt / ArtifactStore state.

## Roles

The **coordinator** is the only harness role that talks to OmniCreator for parallel-work control. It calls `agent_work_graph`, `agent_work_prepare`, and `agent_work_commit`, keeps the current project/work IDs, serializes canonical commit batches, and re-inspects after every commit.

A **worker** receives one prepared descriptor and performs bounded external work such as finding a stock asset, generating a visual, or producing voice audio. A worker returns a candidate result to the coordinator. A worker must not call `agent_work_commit`, edit the Data Root, mutate SQLite/ArtifactStore state, mark workflow steps complete, or invent canonical IDs.

The machine-readable copy of this boundary is `contract.json`.

## Reference loop

```text
Coordinator
   |
   +-- agent_work_graph(project)
   |
   +-- agent_work_prepare(project, work_id) x N
   |
   +-- dispatch bounded workers --------------------+
   |                                                |
   |       visual worker(s)        voice worker(s)  |
   |            |                       |            |
   |         candidate results returned to coordinator
   |                                                |
   +-- agent_work_commit(batch)  <------------------+
   |
   +-- agent_work_graph(project) again
   |
   +-- retry/recover remaining current work
   |
   `-- ProductionPack / Resolve only after canonical fan-in
```

Recommended starting concurrency is 4 workers total. This number is harness-local policy, not persisted OmniCreator workflow truth. Lower it for rate-limited providers or expensive GPU/TTS backends. Raise it only when the external services and machine can support the load.

## Script -> ScenePlan -> Visual + Voice -> QA -> ProductionPack

1. Create/select the project and inspect canonical state.
2. Produce or import Script/Content through the normal typed Content control. Do not dispatch production workers before the Content dependency is canonically satisfied.
3. Produce or import ScenePlan/storyboard through the typed ScenePlan control.
4. Call `agent_work_graph` and choose only items whose canonical readiness allows external execution.
5. Call `agent_work_prepare` immediately before dispatch for each selected `work_id`.
6. Fan out independent visual work after ScenePlan and voice work after Content. Visual and voice workers may run concurrently when their dependencies are ready.
7. Workers return candidate outputs and provenance to the coordinator. Worker completion is not canonical success yet.
8. The coordinator commits a bounded batch through `agent_work_commit`. Canonical writes remain serialized by the Data Root writer lease even when external work ran in parallel.
9. Immediately re-run `agent_work_graph`. Never infer success from the worker transcript.
10. Resolve missing/failed/stale units with current descriptors. Then inspect Review Center and production recovery before ProductionPack/export.

## Retry and reconnect

Worker crash: keep the candidate absent and re-inspect before redispatch. Reuse a prepared descriptor only while current canonical state still exposes the same work identity.

Coordinator reconnect: start by re-inspecting `agent_work_graph`. Do not replay a cached descriptor blindly. Completed results should disappear from READY work or be reflected through canonical selection/state.

`stale_input`: discard the old candidate, re-inspect, prepare the current work item, and run again. Never force an obsolete artifact into current state.

Duplicate/reconnect delivery: send it only through `agent_work_commit` and trust the canonical idempotency/duplicate response. Never deduplicate by directly editing storage.

`writer_conflict`: back off and retry canonical commit through the coordinator. Do not open a second writable OmniCreator instance to bypass the single-writer lease.

Provider/capability failure: preserve the failure truthfully, then choose an applicable manual/external/recovery path. Never convert an unavailable provider into synthetic success.

## Result batching

Batch independent completed candidates to reduce control-plane chatter, but keep batches bounded. A practical default is one small group of completed visual/voice items, then re-inspect before committing the next group. This keeps stale detection close to execution and makes recovery easier to reason about.

Do not hold dozens of completed candidates while the project continues mutating. Freshness beats giant batches.

## Harness-specific recipes

- Codex: `agent-harness/codex/WORKER-POOL.md`
- Claude Code: `agent-harness/claude/WORKER-POOL.md`
- Google Antigravity: `agent-harness/antigravity/WORKER-POOL.md`

All three recipes intentionally share the same OmniCreator semantics. Vendor-specific agent/subagent syntax may evolve, but no vendor is allowed to create a second canonical scheduler or database.
