---
name: omnicreator
description: Safely control an OmniCreator creator project through its canonical MCP or CLI surface, including automatic, mixed manual, recovery, and ProductionPack flows.
---

# OmniCreator agent control

Use OmniCreator as a control plane over its canonical Project / WorkflowStep / Job / Attempt / ArtifactStore state. Prefer MCP when available. Use the CLI only as the equivalent typed transport when the harness is better suited to shell execution.

## Hard rules

1. Inspect before mutating. Start with `workspace_status` / `projects_list` and then `project_get`, `workflow_status`, `creator_state`, `review_list`, or `runtime_status` as needed.
2. Use project, step, scene, segment, artifact, job, attempt, and recovery identifiers returned by OmniCreator. Never invent canonical IDs.
3. After every mutation, re-read the relevant status before choosing the next action.
4. Honor `read_only`, `writer_conflict`, `blocked`, `provider_unavailable`, and `capability_unavailable`. Do not work around canonical guards.
5. Prefer `creator_start_or_resume` or CLI `creator resume` for an existing project. Do not recreate a project just because execution is blocked.
6. Automatic execution being OFF does not mean SKIPPED or SUCCEEDED. Satisfy the step through the corresponding canonical manual/external control, then re-read workflow state.
7. Never edit OmniCreator SQLite, Data Root internals, ArtifactStore files, or workflow state directly. Never use a generic filesystem or shell bypass to fabricate state.
8. Never claim provider, plugin, TTS, GPU, or ComputeProvider success unless OmniCreator reports canonical success/result state.
9. Keep provider settings machine-local. Never put API keys, bearer tokens, or raw provider credentials in Data Root, project state, prompts, MCP payloads, or checked-in config.
10. When recovery is required, inspect Review Center/recovery first and invoke only an applicable typed recovery/manual/external action.

## MCP tool selection

Inspection tools:
- `workspace_status`
- `projects_list`
- `project_get`
- `workflow_status`
- `creator_state`
- `review_list`
- `runtime_status`
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
- `production_control`

Project deletion is destructive. Only request it when explicitly required and use its confirmation contract.

## Default control loop

1. Inspect workspace and projects.
2. Create a project only if the intended project does not already exist.
3. Inspect project + workflow + creator state.
4. Start/resume creator execution.
5. If execution succeeds, inspect state again and continue/resume until the next canonical boundary.
6. If execution is blocked, inspect `review_list`, workflow state, and the stage-specific status/recovery view.
7. If AUTO is OFF or a runtime capability is unavailable, use the matching manual/external control rather than enabling a hidden fallback.
8. Re-read status after the mutation.
9. Assemble/rebuild ProductionPack only when prerequisites are canonically satisfied.
10. Verify production/export status before reporting completion.

## Flow patterns

### All automatic

Inspect -> create/select project -> `creator_start_or_resume` -> inspect -> resume as required. If a provider/plugin/compute blocker appears, report it truthfully or switch to an explicitly available manual/external path.

### Mixed automatic/manual

Inspect -> turn a specific step AUTO OFF with `workflow_set_step_auto` -> provide that stage through its canonical manual/external tool -> verify the step remains AUTO OFF but is satisfied -> `creator_start_or_resume` -> inspect again.

### Fully manual

Keep automatic execution OFF where desired. Supply Content, ScenePlan, visuals, voice/timing, and any external generated result through typed controls. Then assemble ProductionPack/export through `production_control`. Never write artifacts directly into canonical storage.

### Recovery

Inspect `review_list` and `production_control` recovery/status -> choose only an applicable repair/manual/external action -> verify recovery state -> resume/rebuild -> verify ProductionPack/export.

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
omnicreator --data-root <path> --json creator resume --project <id>
```

For complex manual/external mutations, use the documented typed `--stdin` or `--input-file` payload path instead of inventing ad-hoc flags.

## Completion standard

Do not report a flow complete from an agent transcript alone. Completion requires the latest OmniCreator inspection to show the intended canonical result, such as a valid ProductionPack/export state, with no unresolved blocker relevant to the requested outcome.
