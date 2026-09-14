# Phase 18 Roadmap — Agent/Application Control Plane

Authoritative GitHub tracking: umbrella #99 and slices #100 through #105.

This document records repository-level architecture/order only. GitHub issue state and verified CI evidence remain the delivery authority.

## Goal

Make Desktop, CLI and future MCP adapters control the same canonical OmniCreator Project / WorkflowStep / Job / Attempt / ArtifactStore state without duplicating transport-specific workflow logic.

```text
Desktop
   \
CLI ----> Application Control Service ----> omnicreator-core
   /
MCP
```

The control plane does not own a second scheduler, database, workflow engine or artifact truth.

## Ordered slices

| Slice | Tracking | Scope | Current state |
| --- | --- | --- | --- |
| P0 | #100 | Shared Application Control Service + typed operation contracts | DONE / VERIFIED — PR #106 + corrective PR #107; post-merge CI #490 PASS |
| P1 | #101 | Single-binary CLI + deterministic JSON output | DONE / VERIFIED — PR #108; post-merge CI #501 PASS |
| P2 | #102 | MCP server v1 over stdio | IN PROGRESS — `feat/phase18-p2-mcp-stdio` |
| P3 | #103 | Provider-neutral LLM abstraction + OpenRouter direct | NOT STARTED |
| P4 | #104 | Agent skills/config/examples | NOT STARTED |
| P5 | #105 | MCP Tasks, security, E2E parity, docs and hardening | NOT STARTED |

Default sequence remains `P0 -> P1 -> P2 -> P3 -> P4 -> P5`.

## P0 verified checkpoint

P0 introduced the transport-neutral `omnicreator-application` crate while keeping `omnicreator-core` authoritative for domain/workflow semantics.

Verified properties:

- writable control services can only be built from the existing single-writer `WorkspaceSession`;
- read-only inspection uses the existing read-only StateStore and application mutations fail with typed `read_only`;
- versioned typed requests/results/errors are serde-safe for later JSON/MCP adapters;
- project/workflow/run/manual/external/recovery/production operations delegate to canonical core paths;
- Creator Start/Resume sequencing and Phase 17 automatic-execution gates live in `CreatorRunControlServiceV1`, not Tauri;
- machine-local LLM/plugin/ComputeProvider execution is injected through `CreatorRunRuntimeV1`;
- Desktop Start/Resume is a thin adapter over the shared controller;
- provider-free full-manual acceptance reaches ProductionPack and Resolve/DaVinci export through canonical Jobs, Attempts and ArtifactStore verification;
- response regressions reject unintended Data Root path and credential leakage.

P0 final evidence:

- final corrective head `e7dfd194563b0ff65b9b8be934420d8b3dcd26a5`
- exact-head CI #489 / run `34770271377`: PASS
- guarded squash merge `3aa9fd936525664bd813bc61532f079da178fc43`
- merge tree `3d9a35b86117050219a977cd35f9f4a3cd964b0a`
- post-merge CI #490 / run `34770523127`: PASS

Detailed P0 contract/boundary documentation is in `docs/16-application-control-service.md`.

## P1 verified checkpoint

P1 added one Rust CLI binary, `omnicreator`, as a transport adapter over the P0 Application Control Service.

Verified P1 evidence:

- final exact head `4a07af1aba964823dc48b9795ec671a66d96bf0f`
- exact-head/merge tree `ddd72257abd132061f71441d599e390320662f84`
- exact-head CI #500 / run `34799391242`: PASS
- guarded squash merge/current verified main `b790feae93ccc0fa14619d3d1ec696ab0767178a`
- post-merge CI #501 / run `34799662666`: PASS

Verified properties include deterministic JSON + stable exit codes, read-only enforcement, canonical manual/external takeover through Resolve export, CLI/Application projection parity, project-scoped Review Center, AUTO OFF + manual satisfaction, and truthful provider/capability blockers.

Command and JSON details are documented in `docs/18-cli-control.md`.

## P2 architecture checkpoint

P2 exposes the same application boundary as a standards-based local MCP server from the existing `omnicreator` binary.

Current implementation decisions:

- branch starts exactly from verified P1 main `b790feae93ccc0fa14619d3d1ec696ab0767178a`, tree `ddd72257abd132061f71441d599e390320662f84`;
- protocol target is stable MCP `2026-07-28`;
- implementation uses the official Rust SDK `rmcp 3.3.0` and stdio transport;
- because `rmcp 3.3.0` requires Rust 1.88, workspace MSRV is intentionally raised from 1.80 to 1.88 and all existing Rust/Desktop regressions must prove the bump safe;
- one `omnicreator` executable exposes both normal CLI mode and `mcp serve`; stdout stays protocol-only in MCP mode;
- MCP tools are capability-oriented adapters over `ApplicationControlService` / `CreatorRunControlServiceV1`, never direct SQLite/StateStore mutations;
- writer calls acquire the same `WorkspaceSession`; `--read-only` uses the same application-level guard;
- successes use structured MCP results; canonical typed application failures use structured MCP tool errors rather than opaque transport failure;
- Creator Start/Resume uses the shared P0 controller and truthful local runtime blockers;
- resources/prompts are intentionally deferred in P2 unless they provide distinct context-efficiency value beyond typed inspection tools;
- remote Streamable HTTP remains deferred until authenticated/authorized safely;
- P3 OpenRouter/provider-neutral LLM work, P4 vendor configs and P5 MCP Tasks/hardening remain out of scope.

Detailed P2 behavior is documented in `docs/19-mcp-control.md`.

## P2 verification gate

P2 remains IN PROGRESS until:

1. an official MCP client connects to the real `omnicreator ... mcp serve` child process over stdio and discovers tools;
2. MCP full-manual acceptance toggles AUTO OFF and reaches canonical ProductionPack + Resolve export;
3. MCP projection matches direct Application Control Service state;
4. read-only mutation returns structured `read_only`;
5. automatic Start/Resume returns truthful provider/capability blockers without Desktop or fake success;
6. response leakage regressions remain clean;
7. the Rust 1.88 MSRV bump leaves Rust/CLI/Plugins/Desktop CI green;
8. exact-head CI passes before guarded squash merge;
9. post-merge CI passes on the exact merge SHA before #102 is marked DONE / VERIFIED.

P3 implementation must not start during the P2 verification run.
