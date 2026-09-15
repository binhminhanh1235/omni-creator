# Claude Code worker-pool recipe

Use the main Claude Code session as the OmniCreator coordinator. Subagents or parallel tasks are external workers, not canonical workflow owners.

## Coordinator contract

The coordinating session must inspect `agent_work_graph`, select current executable work, and call `agent_work_prepare` before delegating. Keep the default pool bounded to 4 workers unless provider or machine capacity justifies another local limit.

Each delegated worker receives only the canonical `project_id`, `work_id`, typed `external` descriptor, and the external task. It returns a candidate artifact/result plus truthful provenance to the coordinator. Workers do not call `agent_work_commit`, do not edit Data Root files, and do not mark workflow stages complete.

The coordinator alone calls `agent_work_commit`, then immediately re-inspects `agent_work_graph`. A worker transcript is never evidence of canonical success.

## Production shape

Commit Script/Content first, then commit ScenePlan/storyboard. Once dependencies are satisfied, visual scene workers and voice segment workers may execute in parallel. The coordinator performs bounded fan-in commits and uses Review Center/recovery for missing, failed, or stale units before ProductionPack/Resolve export.

## Recovery

After a Claude Code reconnect or context restart, rebuild the queue from `agent_work_graph`; do not replay a cached queue as truth. For `stale_input`, discard the candidate and re-prepare. For duplicate/reconnect delivery, submit only through canonical commit and trust OmniCreator's idempotency response. For `writer_conflict`, retry later through the coordinator rather than opening a second writer.

See `agent-harness/worker-pool/contract.json` and `agent-harness/worker-pool/README.md` for the shared contract.
