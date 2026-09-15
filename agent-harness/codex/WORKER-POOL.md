# Codex worker-pool recipe

Use Codex as the coordinator and bounded external workers as disposable executors. OmniCreator remains canonical truth.

## Coordinator prompt contract

Tell the coordinating Codex session to:

1. inspect `agent_work_graph` for the selected project;
2. choose only currently executable visual/voice `work_id` values;
3. call `agent_work_prepare` immediately before dispatch;
4. launch at most 4 workers by default;
5. give each worker only the canonical `project_id`, `work_id`, returned `external` descriptor, and the external-service task;
6. require workers to return candidate result/provenance only, never mutate OmniCreator state;
7. collect completed candidates and call `agent_work_commit` from the coordinator only;
8. re-run `agent_work_graph` after every commit batch;
9. discard and re-prepare stale work instead of forcing cached results;
10. continue through Review Center/QA and ProductionPack only from canonical state.

## Suggested worker split

- Script/Content reasoning remains serial until the chosen script is committed.
- ScenePlan/storyboard remains serial until committed.
- After Content + ScenePlan readiness, use independent workers for scene visuals and voice segments.
- Keep expensive generated-image or voice jobs in smaller pools when provider/GPU capacity is limited.

A worker may use Pexels, another visual provider, OmniVoiceStudio, or a manual/external path according to the descriptor and the task it was assigned. It must not decide that OmniCreator state is complete.

## Reconnect

If the coordinator session restarts, do not trust the old worker queue. Inspect `agent_work_graph` again, reconcile returned candidate outputs with current `work_id` values, and commit only candidates that still map to current work. If OmniCreator reports `stale_input`, discard that candidate and prepare current work again.

See `agent-harness/worker-pool/contract.json` and `agent-harness/worker-pool/README.md` for the vendor-neutral contract.
