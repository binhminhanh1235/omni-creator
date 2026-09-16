---
name: omnicreator
description: Safely control an OmniCreator creator project through its canonical MCP or CLI surface, including automatic, mixed manual, recovery, parallel worker-pool, and ProductionPack flows.
---

# OmniCreator agent control

Use OmniCreator as a control plane over its canonical Project / WorkflowStep / Job / Attempt / ArtifactStore state. Prefer MCP when available. Use the CLI only as the equivalent typed transport when the harness is better suited to shell execution.

## Hard rules

1. Inspect before mutating. Start with `workspace_status` / `projects_list` and then `project_get`, `workflow_status`, `creator_state`, `review_list`, or `runtime_status` as needed.
2. Use project, step, scene, segment, artifact, job, attempt, recovery, and `work_id` identifiers returned by OmniCreator. Never invent canonical IDs.
3. After every mutation, re-read the relevant status before choosing the next action.
4. Honor `read_only`, `writer_conflict`, `blocked`, `provider_unavailable`, `capability_unavailable`, and `stale_input`. Do not work around canonical guards.
5. Prefer `creator_start_or_resume` or CLI `creator resume` for an existing project. Do not recreate a project just because execution is blocked.
6. Automatic execution being OFF does not mean SKIPPED or SUCCEEDED. Satisfy the step through the corresponding canonical manual/external control, then re-read workflow state.
7. Never edit OmniCreator SQLite, Data Root internals, ArtifactStore files, or workflow state directly. Never use a generic filesystem or shell bypass to fabricate state.
8. Never claim provider, plugin, TTS, GPU, or ComputeProvider success unless OmniCreator reports canonical success/result state. Likewise, never claim worker or external-service success until OmniCreator accepts the corresponding canonical result.
9. Keep provider settings machine-local. Never put API keys, bearer tokens, or raw provider credentials in Data Root, project state, prompts, MCP payloads, or checked-in config.
10. When recovery is required, inspect fan-in QA and Review Center/recovery first, then production recovery as needed, and invoke only an applicable typed recovery/manual/external action.

## MCP tool selection

Inspection tools:
- `workspace_status`
- `projects_list`
- `project_get`
- `workflow_status`
- `creator_state`
- `review_list`
- `runtime_status`
- `agent_work_graph`
- `agent_fan_in_qa`
- `agent_work_prepare`
- `visual_control` with `status`
- `voice_control` with `status`
- `production_control` with `status` or `recovery`

Mutation/control tools:
- `project_create`
- `project_update`
- `workflow_set_step_auto`
- `creator_start_or_resume`
- `content_control`
- `scene_plan_control`
- `visual_control`
- `voice_control`
- `agent_work_commit`
- `production_control`

Project deletion is destructive. Only request it when explicitly required and use its confirmation contract.

## Default control loop

1. Inspect workspace and projects.
2. Create a project only if the intended project does not already exist.
3. Inspect project + workflow + creator state.
4. Start/resume creator execution.
5. If execution succeeds, inspect state again and continue/resume until the next canonical boundary.
6. If execution is blocked, inspect `agent_fan_in_qa`, `review_list`, workflow state, and the stage-specific status/recovery view.
7. If AUTO is OFF or a runtime capability is unavailable, use the matching manual/external control rather than enabling a hidden fallback.
8. Re-read status after the mutation.
9. Before ProductionPack, inspect `agent_fan_in_qa`; assemble/rebuild only when canonical fan-in is verified.
10. Verify production/export status before reporting completion.

## Flow patterns

### All automatic

Inspect -> create/select project -> `creator_start_or_resume` -> inspect -> resume as required. If a provider/plugin/compute blocker appears, report it truthfully or switch to an explicitly available manual/external path.

### Mixed automatic/manual

Inspect -> turn a specific step AUTO OFF with `workflow_set_step_auto` -> provide that stage through its canonical manual/external tool -> verify the step remains AUTO OFF but is satisfied -> `creator_start_or_resume` -> inspect again.

### Fully manual

Keep automatic execution OFF where desired. Supply Content, ScenePlan, visuals, voice/timing, and any external generated result through typed controls. Then assemble ProductionPack/export through `production_control`. Never write artifacts directly into canonical storage.

### Recovery

Inspect `agent_fan_in_qa`, `review_list` and `production_control` recovery/status -> choose only an applicable repair/manual/external action -> verify recovery state -> resume/rebuild -> verify ProductionPack/export.

## Parallel worker pool

Use one harness coordinator for canonical OmniCreator control and bounded workers only for external execution. The coordinator calls `agent_work_graph`, selects current executable units, and calls `agent_work_prepare` immediately before dispatch. A worker receives only canonical `project_id`, `work_id`, the typed `external` descriptor, and its external task.

Workers never call `agent_work_commit`. They never edit SQLite, ArtifactStore, Data Root internals, workflow state, selected artifacts, or completion flags. A worker returns a candidate result plus truthful provenance to the coordinator. Worker completion is not canonical success.

The coordinator may fan out independent scene visual work and voice-segment work concurrently after their canonical dependencies are satisfied. Start with at most 4 workers unless provider/GPU/TTS capacity calls for a smaller local limit. Concurrency is harness-local policy, never persisted workflow truth.

The coordinator alone calls `agent_work_commit`, serializing canonical commit batches through the Data Root writer lease. Immediately run `agent_work_graph` again after every commit batch. Use `agent_fan_in_qa` after a wave completes and before ProductionPack. Do not infer state from worker transcripts.

On reconnect, rebuild the queue from the current graph. Already verified canonical artifacts that still verify at the active Data Root remain reusable and must not be recomputed just because the harness restarted. On `stale_input`, discard the candidate and call `agent_work_prepare` again for current work. On duplicate delivery, submit only through canonical commit and trust the canonical idempotency response. On `writer_conflict`, back off and retry through the coordinator rather than opening another writer.

Reference package: `agent-harness/worker-pool/contract.json`, `agent-harness/worker-pool/README.md`, plus vendor recipes under `agent-harness/{codex,claude,antigravity}/WORKER-POOL.md`.

## Parallel production recipes

Parallel execution, serialized canonical commits. Workers may render/search/fetch concurrently, but only the coordinator may commit accepted results into OmniCreator.

For stock visual work, follow the canonical Studio Pack route and actual runtime capability. Pexels is preview-first: run `visual.resolve`, inspect metadata/previews and select a candidate, then fetch only that selection with `visual.fetch_selected`. Never speculatively download every search result. A Pexels image/video is stock media, not an `ExternalGeneratedVisualRequestV1`; do not submit stock video through the generated-still external contract.

For generated stills, use only the current prepared `visual.generate` descriptor from `agent_work_prepare`, preserve its `request_sha256`, and commit the supported still result through `agent_work_commit`. On `stale_input`, discard the obsolete candidate and prepare again.

For READY voice work, prepare the canonical `tts.generate` descriptor and pass it to OmniVoiceStudio as an external renderer. OmniVoiceStudio must return supported audio plus timing for the same segment. Commit with truthful source label `omnivoice-studio`; renderer completion is not canonical success until OmniCreator accepts it.

Provider concurrency, retry/rate-limit state, credentials, paths and GPU/TTS capacity are machine-local or harness-local. They are never portable Project truth. If Pexels, a generated provider, OmniVoiceStudio, or compute is unavailable, use the canonical manual/external fallback without rewriting the failed provider Attempt into success.

Reference package: `agent-harness/parallel-production/contract.json`, `agent-harness/parallel-production/README.md`, and `agent-harness/parallel-production/README.vi.md`.

## Fan-in QA and recovery

Use `agent_fan_in_qa` as the canonical pre-ProductionPack health projection. It combines visual/voice work state with Production Recovery artifact verification without creating a worker-status database.

Interpretation:
- `ready_work_ids`: visual/voice units that can still be dispatched;
- `needs_review_work_ids`: canonical visual/voice work whose Job state needs review/recovery;
- `unhealthy_canonical_units`: units with a physical selected-artifact defect, including `NEEDS_REVIEW` caused by missing/invalid artifacts and the defensive case of `SATISFIED` with a non-verified recovery artifact;
- `fan_in_verified`: every discovered visual + voice unit is SATISFIED and every selected production input verifies at the current Data Root;
- `production_pack_ready`: canonical fan-in is complete and ProductionPack can be assembled;
- `production_pack_satisfied`: ProductionPack is already canonical success.

`read_only=true` is an access flag, not a reason to hide the recommended next action. A read-only agent may inspect the same QA/recovery guidance, but mutation still has to go through an authorized writer.

For `repair_visual` or `repair_voice_bundle`, inspect `production_control` recovery and use the matching typed repair path. For `review`, inspect Review Center and current work graph before retrying or replacing. For `dispatch_external`, prepare current work again immediately before dispatch. For `assemble_production_pack`, re-check QA and then use canonical ProductionPack controls.

After Data Root move/rebind or process restart, cold-inspect `agent_work_graph` and `agent_fan_in_qa`. Never carry absolute old paths, worker claims, or in-memory completion state across the restart.

## Provider selection

LLM execution is optional for manual flows. For automatic Content/Scene intelligence, configure exactly one machine-local provider path:
- direct OpenRouter / OpenAI-compatible: `--llm-provider-config <path>`
- legacy-compatible LLMGateway: either the provider-neutral config or `--llmgateway-config <path>`

Provider config files store environment-variable names, never secret values. See `agent-harness/providers/` and `docs/20-llm-providers.md`.

## CLI fallback

The CLI is the same control boundary, not a different workflow engine. Always include `--data-root <path>` and prefer `--json` for agents. Useful equivalents include:

```text
omnicreator --data-root <path> --json workspace status
omnicreator --data-root <path> --json project list
omnicreator --data-root <path> --json project show --project <id>
omnicreator --data-root <path> --json workflow status --project <id>
omnicreator --data-root <path> --json creator state --project <id>
omnicreator --data-root <path> --json review list --project <id>
omnicreator --data-root <path> --json work graph --project <id>
omnicreator --data-root <path> --json work qa --project <id>
omnicreator --data-root <path> --json work prepare --project <id> --work <work-id>
omnicreator --data-root <path> --json creator resume --project <id>
```

For `work commit` and other complex manual/external mutations, use the documented typed `--stdin` or `--input-file` payload path instead of inventing ad-hoc flags.

## Completion standard

Do not report a flow complete from an agent transcript alone. Completion requires the latest OmniCreator inspection to show the intended canonical result, such as `fan_in_verified=true` followed by a valid ProductionPack/export state, with no unresolved blocker relevant to the requested outcome.
