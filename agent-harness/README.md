# OmniCreator agent harness package

This package wires Codex, Claude Code, and Google Antigravity to the same local `omnicreator` executable. It does not introduce an agent-specific workflow database or a second scheduler.

## Prerequisites

1. Build/install the Rust `omnicreator` binary and make it available on `PATH`, or replace `command = "omnicreator"` / `"command": "omnicreator"` with an absolute binary path.
2. Choose one existing writable OmniCreator Data Root. Replace `/ABSOLUTE/PATH/TO/OMNICREATOR_DATA_ROOT` in the examples with its absolute machine-local path.
3. Keep provider credentials in environment variables. Never put secret values in these files or in the Data Root.

A provider is optional for fully manual flows. For automatic Content/Scene intelligence, add the machine-local provider config arguments before `mcp serve`:

```text
omnicreator --data-root <path> --llm-provider-config <provider.json> mcp serve
```

Use `providers/openrouter.json.example` with `OPENROUTER_API_KEY` in the process environment for direct OpenRouter, or `providers/llmgateway.json.example` with `LLMGATEWAY_API_KEY` for LLMGateway. The files contain environment-variable names only.

## Codex

OmniCreator's repository skill is already placed at `.agents/skills/omnicreator/SKILL.md`.

For MCP, copy the `mcp_servers.omnicreator` block from `codex/config.toml.example` into the Codex config that should own this server, then replace the Data Root placeholder. The equivalent CLI registration is:

```text
codex mcp add omnicreator -- omnicreator --data-root /ABSOLUTE/PATH/TO/OMNICREATOR_DATA_ROOT mcp serve
```

After registration, ask Codex to inspect OmniCreator before making changes. Do not grant it a separate direct SQLite/ArtifactStore workflow.

## Claude Code

OmniCreator's project skill is at `.claude/skills/omnicreator/SKILL.md` and is intentionally kept byte-identical to the `.agents` copy.

Copy `claude/.mcp.json.example` to the project's `.mcp.json`, replace the Data Root placeholder, and approve the project MCP server when Claude Code asks. Keep the MCP file project-local only when that is the intended trust boundary.

## Google Antigravity

Antigravity shares the repository skill at `.agents/skills/omnicreator/SKILL.md`.

Copy the `mcpServers.omnicreator` entry from `antigravity/mcp_config.json.example` into the Antigravity MCP configuration you use, or into a supported project/workspace MCP config, then replace the Data Root placeholder. The server is local stdio and remains the same `omnicreator ... mcp serve` process used by the other harnesses.

## CLI-only harnesses

The CLI and MCP are equivalent adapters over the Application Control Service. Agents that prefer shell commands should use deterministic JSON output:

```text
omnicreator --data-root <path> --json workspace status
omnicreator --data-root <path> --json project list
omnicreator --data-root <path> --json project show --project <project-id>
omnicreator --data-root <path> --json workflow status --project <project-id>
omnicreator --data-root <path> --json creator state --project <project-id>
omnicreator --data-root <path> --json review list --project <project-id>
omnicreator --data-root <path> --json creator resume --project <project-id>
```

For Content, ScenePlan, visuals, voice/timing, external handoff, and recovery mutations, use the documented typed payload input (`--stdin` or `--input-file`) rather than editing Data Root files.

### Phase 19 parallel work

Harnesses that want to fan out visual/voice execution should inspect the canonical graph instead of reverse-engineering jobs or keeping their own queue:

```text
omnicreator --data-root <path> --json work graph --project <project-id>
omnicreator --data-root <path> --json work prepare --project <project-id> --work <work-id>
```

Both commands are read-only projections and do not acquire the canonical writer lease. The returned `work_id`, dependency state, input hash, selected artifact IDs, and external descriptor are the authority for dispatch.

Workers may run concurrently outside OmniCreator. Return completed visual/voice results through one canonical batch ingress:

```text
omnicreator --data-root <path> --json work commit --stdin
```

or `--input-file <batch.json>`. The commit is serialized through the shared writer lease and preserves stale-input rejection, duplicate/reconnect reuse, per-item failure reporting, and explicit replacement semantics.

MCP clients use the equivalent tools `agent_work_graph`, `agent_work_prepare`, and `agent_work_commit`. Graph/prepare stay read-only even on a writable server. Commit requires mutation access. These bounded operations are synchronous; durable MCP Tasks remain for actually long-running operations such as `creator_start_or_resume`.

A safe worker loop is: inspect graph -> prepare returned work IDs -> fan out externally -> batch commit completed results -> inspect graph again. Do not treat a local worker's completion as canonical success until OmniCreator accepts the result.

## Safe flow recipes

### All-auto

Inspect workspace/project/runtime -> create/select project -> Start/Resume -> inspect -> resume as canonical state permits. A provider/plugin/compute blocker is a blocker, never an implicit success.

### Mixed auto/manual

Inspect -> set exactly one stage AUTO OFF -> inspect -> provide the canonical manual/external result for that stage -> inspect that the stage remains AUTO OFF but is satisfied -> resume -> inspect.

### Fully manual

Keep desired stages AUTO OFF. Provide Content, ScenePlan, visual, voice/timing and external generated outputs through typed controls. Finish through ProductionPack/export controls. No LLM, visual provider, TTS, or GPU is required if all required results are supplied canonically.

### Recovery

Inspect Review Center and stage/production recovery -> choose an applicable typed repair/manual/external action -> inspect -> resume/rebuild -> inspect ProductionPack/export before reporting completion.

## Blocked workflow fixture

`fixtures/blocked-workflow-recovery.json` is the reference agent transcript contract for a no-provider run. It deliberately observes `provider_unavailable`, turns Content AUTO OFF, supplies manual canonical results, re-inspects after mutations, and reaches ProductionPack only through typed controls.

CI validates that every tool named by this fixture exists on the real MCP server and that the skill/config examples do not drift away from the executable surface. Existing MCP E2E tests execute the provider-free manual flow through ProductionPack/export.

## Sources for harness configuration formats

- Codex MCP: https://developers.openai.com/codex/mcp
- Codex skills: https://developers.openai.com/codex/skills
- Claude Code MCP: https://docs.anthropic.com/en/docs/claude-code/mcp
- Claude Code skills: https://docs.anthropic.com/en/docs/claude-code/skills
- Google Antigravity MCP/skills: use the current Antigravity/Gemini documentation for your installed version; the checked-in example intentionally uses only the common local `mcpServers` command/args shape.
