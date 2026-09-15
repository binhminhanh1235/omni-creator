# Google Antigravity worker-pool recipe

Use the primary Antigravity session/workflow as the OmniCreator coordinator. Parallel agents are external workers that execute prepared tasks; they are not a second OmniCreator scheduler.

## Coordinator contract

Inspect `agent_work_graph`, choose only currently executable work IDs, and call `agent_work_prepare` immediately before dispatch. Start with at most 4 concurrent workers. The limit is local harness policy and must not be written into OmniCreator project state as workflow truth.

Give each worker only the canonical `project_id`, `work_id`, typed `external` descriptor, and its external execution task. A worker returns a candidate result/provenance. It must not call `agent_work_commit`, mutate SQLite/ArtifactStore/Data Root internals, or claim canonical success.

The coordinator collects candidates, calls `agent_work_commit` in bounded serialized batches, and immediately runs `agent_work_graph` again. Any next dispatch comes from the refreshed graph.

## Production shape

Script/Content and ScenePlan/storyboard establish the dependency boundary. After they are canonically satisfied, visual work by scene and voice work by segment may fan out together. After fan-in, use Review Center/recovery to resolve missing, failed, stale, or replaced results before ProductionPack and Resolve export.

## Recovery

On reconnect, discard the old queue as authority and reconstruct it from the current graph. `stale_input` means the candidate is obsolete and must be regenerated from a newly prepared descriptor. Duplicate result delivery goes through canonical commit. `writer_conflict` means retry the coordinator commit later, never bypass the Data Root lease.

See `agent-harness/worker-pool/contract.json` and `agent-harness/worker-pool/README.md` for the shared contract.
