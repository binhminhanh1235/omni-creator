# Phase 18 P4 — Agent harness packages

Tracking: Phase 18 umbrella #99, P4 #104.

## Purpose

P4 makes OmniCreator discoverable and safe to use from Codex, Claude Code, Google Antigravity, and CLI-oriented agent harnesses without creating a second source of workflow truth.

All harnesses invoke the same Rust executable and the same Application Control Service:

```text
Agent harness
   |
   +-- local MCP stdio: omnicreator ... mcp serve
   `-- CLI JSON:        omnicreator ... --json <resource> <verb>
              |
      Application Control Service
              |
      canonical OmniCreator state
```

## Checked-in package

- `.agents/skills/omnicreator/SKILL.md` — repository skill for Codex and Antigravity.
- `.claude/skills/omnicreator/SKILL.md` — project skill for Claude Code; CI requires it to be byte-identical to the `.agents` copy.
- `agent-harness/codex/config.toml.example` — local stdio MCP configuration for Codex.
- `agent-harness/claude/.mcp.json.example` — project MCP configuration for Claude Code.
- `agent-harness/antigravity/mcp_config.json.example` — local MCP configuration for Antigravity.
- `agent-harness/providers/openrouter.json.example` — secret-free direct OpenRouter machine-local provider profile.
- `agent-harness/providers/llmgateway.json.example` — secret-free LLMGateway machine-local provider profile.
- `agent-harness/fixtures/blocked-workflow-recovery.json` — inspect/mutate/re-inspect recovery transcript contract.
- `agent-harness/README.md` — installation and all-auto/mixed/manual/recovery guidance.

## Safety contract

The skill tells every harness to inspect before mutation, reuse returned canonical IDs, honor read-only/writer/provider/capability blockers, re-read state after every mutation, prefer resume/recovery over recreating projects, and never edit SQLite/ArtifactStore internals or fabricate provider/plugin/compute success.

Automatic execution OFF is policy, not completion. A harness must satisfy the step through the corresponding canonical manual/external operation and then verify state again.

## Provider configuration

Provider configuration is machine-local. The examples store only the environment-variable name holding a credential. They do not contain secret values.

Direct OpenRouter and generic OpenAI-compatible execution use the P3 `--llm-provider-config` boundary. LLMGateway remains supported through the same provider-neutral config or the legacy-compatible `--llmgateway-config` flag.

Provider configuration never becomes portable Project / WorkflowStep orchestration truth.

## Verification

`crates/omnicreator-cli/tests/agent_harnesses.rs` verifies:

1. Codex/Antigravity and Claude skill copies cannot drift.
2. Required safety instructions remain present.
3. Claude and Antigravity JSON stdio examples are valid and equivalent.
4. The Codex example names the real `omnicreator --data-root ... mcp serve` invocation.
5. Both provider examples deserialize and validate through the real `LlmProviderConfigV1` schema.
6. Checked-in package text rejects common secret-like markers.
7. Every fixture mutation is immediately followed by inspection.
8. The fixture deliberately preserves `provider_unavailable` rather than converting it to success.
9. An official rmcp client launches the real `omnicreator` child and proves every fixture tool exists on the actual MCP server.

The existing MCP E2E suite remains the executable completion proof for the provider-free manual flow through ProductionPack/export. P4 adds harness packaging and drift protection, not a vendor-specific workflow runtime.

## Phase 18 P5 durable Tasks guidance

P5 keeps the same harness package and the same `omnicreator ... mcp serve` command. No vendor-specific config change is required. A newer MCP client may additionally declare the `io.modelcontextprotocol/tasks` extension.

When Tasks capability is declared, `creator_start_or_resume` may return a durable Task handle instead of an immediate tool result. The handle is backed by canonical OmniCreator Job/Attempt/Artifact state, not an in-memory agent or MCP database.

Agent behavior for a returned Task must be:

1. retain the returned `taskId`;
2. poll `tasks/get` no faster than the server's suggested `pollIntervalMs`;
3. reconnect and continue using the same `taskId` if the stdio connection disappears;
4. treat `completed` as protocol completion, then inspect the embedded `CallToolResult` because it can legitimately contain `isError=true` for a truthful blocker such as `provider_unavailable`;
5. after terminal task status, inspect the canonical project/workflow state before deciding the next action;
6. use `tasks/cancel` only when cancellation is actually intended; cancellation does not roll back workflow changes or verified artifacts already committed;
7. use canonical manual/external takeover and creator resume for missing-provider/capability recovery rather than trying to edit task storage or answer `tasks/update` requests.

Clients that do not declare Tasks capability keep the synchronous MCP behavior verified in P2/P4. The harness therefore remains backwards compatible.

For detailed operator semantics, restart behavior, Data Root portability and cancellation rules, see `docs/22-mcp-tasks-operations.md` and `docs/22-mcp-tasks-operations.vi.md`.

## Non-goals

P4 itself did not add remote MCP HTTP, long-running MCP Tasks, an agent database, arbitrary shell tools, direct canonical-storage mutation, or P5 security/task hardening. P5 adds durable Tasks hardening without changing the local-stdio, no-shadow-state and no-arbitrary-shell boundaries.
