# MCP Tasks operations

This guide covers the Phase 18 P5 durable MCP Tasks behavior for OmniCreator. It complements `docs/19-mcp-control.md` and the agent packages in `agent-harness/`.

## Transport and protocol

OmniCreator keeps local `stdio` as the Phase 18 control-plane transport and targets MCP `2026-07-28` through `rmcp 3.3.0`.

The server advertises the `io.modelcontextprotocol/tasks` extension. A tool call is materialized as a Task only when the connected client declares Tasks capability. Clients without that capability continue to receive the existing synchronous MCP tool response.

Phase 18 P5 taskifies `creator_start_or_resume`. The rest of the existing MCP tool surface remains unchanged.

## Canonical task identity

An MCP Task is not stored in an MCP-only database. Its `taskId` is the canonical OmniCreator `Job` ID. The task audit trail uses the existing canonical `Job`, `Attempt`, and `Artifact` records in the Data Root.

This is deliberate:

- there is no `mcp_tasks` shadow database or process-local source of truth;
- a server restart does not erase a completed task;
- moving the portable Data Root preserves completed task state and result artifacts;
- task status can be reconstructed from canonical state after reconnect;
- agent-originated work participates in the same audit and recovery surfaces as other OmniCreator execution.

Completed task output is persisted as a verified `control.task-result.v1` artifact under the project logical namespace. The artifact stores the original MCP `CallToolResult`, so a tool-level error such as `provider_unavailable` is a completed MCP Task whose result has `isError=true`. This follows MCP Tasks semantics and does not fabricate provider success.

## Lifecycle

For a Tasks-capable client:

1. Call `creator_start_or_resume`.
2. The server may return `resultType: "task"` with a durable `taskId`.
3. Poll `tasks/get` using the suggested `pollIntervalMs`.
4. Terminal status is one of `completed`, `failed`, or `cancelled`.
5. A later Tasks-capable client can call `tasks/get` with the same ID after reconnect or restart.

Canonical mapping:

| OmniCreator canonical state | MCP task state |
| --- | --- |
| `READY`, `QUEUED`, `RUNNING` | `working` |
| `SUCCEEDED` | `completed` |
| `CANCELLED` | `cancelled` |
| interrupted/retryable/fatal/stale invalid terminal state | `failed` |

OmniCreator does not currently expose MCP `input_required` task requests. `tasks/update` is therefore rejected. Use the typed manual/external takeover tools and then call creator resume instead.

## Cancellation

`tasks/cancel` is cooperative and canonical. It updates the control Job and active Attempt together to `CANCELLED`; it never marks provider or workflow work as successful.

Cancellation is not a transaction rollback for creator work that was already committed before the cancellation request. Verified artifacts and canonical workflow mutations that already completed remain valid. The cancelled task result is not promoted over the cancelled task state.

A read-only MCP session cannot cancel a task.

## Restart and interruption

On writable MCP server startup, OmniCreator runs its existing canonical interrupted-job reconciliation. Any abandoned `RUNNING` Job/Attempt is moved to the existing retryable reconciliation state instead of being reported as indefinitely working or fabricated as success.

A completed task is not affected by restart reconciliation because its Job/Attempt and verified result artifact are already terminal.

## Portable Data Root

Task state is portable with the Data Root. After stopping the MCP process, the Data Root may be moved or synced according to the normal OmniCreator portability rules. Reconnect the server using the new Data Root path and use the same task ID with `tasks/get`.

Do not copy a live Data Root while a writer is active. The single-writer lease remains authoritative for Desktop, CLI, and MCP.

## Security and privacy

The P5 layer inherits the existing response sanitizer and typed control errors. Operators and agents must not place credentials in project content, task IDs, artifact metadata, or portable provider configuration.

Provider secrets remain machine-local and are referenced only through configured environment-variable names. MCP responses must not expose raw bearer tokens, API-key values, or unintended absolute Data Root paths.

## Compatibility

A client that does not declare the Tasks extension continues to use the Phase 18 P2 synchronous behavior. This allows existing Claude/Codex/other stdio integrations to continue operating while newer clients can opt into durable Task handles.

Local stdio remains the supported Phase 18 transport. Public or unauthenticated remote MCP control is out of scope.

## Operator checks

Before treating a task as successful, inspect its terminal `tasks/get` result and then inspect the corresponding canonical project/workflow status. A `completed` MCP Task can legitimately contain a tool result with `isError=true`, for example a truthful `provider_unavailable` blocker.

For recovery, use Review Center, manual/external takeover, retry/reconciliation, and creator resume. Never edit the SQLite database or task-result artifacts directly.
