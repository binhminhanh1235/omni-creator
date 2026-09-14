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
| P2 | #102 | MCP server v1 over stdio | DONE / VERIFIED — PR #109; post-merge CI #527 PASS |
| P3 | #103 | Provider-neutral LLM abstraction + OpenRouter direct | IN PROGRESS — PR #110 / `feat/phase18-p3-llm-provider-openrouter` |
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

## P2 verified checkpoint

P2 exposes the same application boundary as a standards-based local MCP server from the existing `omnicreator` binary.

Verified implementation decisions:

- protocol target is stable MCP `2026-07-28`;
- implementation uses the official Rust SDK `rmcp 3.3.0` and stdio transport;
- workspace MSRV is intentionally Rust 1.88 because the official SDK requires it;
- one `omnicreator` executable exposes both normal CLI mode and `mcp serve`; stdout stays protocol-only in MCP mode;
- MCP tools are capability-oriented adapters over `ApplicationControlService` / `CreatorRunControlServiceV1`, never direct SQLite/StateStore mutations;
- writer calls acquire the same `WorkspaceSession`; `--read-only` uses the same application-level guard;
- successes use structured MCP results; canonical typed application failures use structured MCP tool errors;
- destructive project delete requires explicit confirmation;
- Creator Start/Resume uses the shared P0 controller and truthful local runtime blockers;
- remote Streamable HTTP remains deferred until authenticated/authorized safely;
- resources/prompts remain deferred where they would merely duplicate typed inspection tools.

P2 final evidence:

- PR #109 final head `e02f3474934e3c1bef242a12d4fa61b3cad10464`
- exact-head CI #526 / run `34805856829`: PASS across Rust / Plugins / Desktop
- guarded squash merge `026871cdeeaae61f69da4c7bde76c19742b01842`
- merge tree `2a6cdc37ee7fa0b2f40adb6afa1c852ea100bf3d`
- post-merge CI #527 / run `34806301978`: PASS across Rust / Plugins / Desktop

Detailed P2 behavior is documented in `docs/19-mcp-control.md`.

## P3 architecture checkpoint

P3 makes LLMGateway one supported creator-intelligence provider instead of the only transport and adds direct OpenRouter through an OpenAI-compatible adapter.

Current architecture:

- `LlmProviderV1` defines provider-neutral chat/model/readiness behavior below shared creator execution;
- `ConfiguredLlmProviderV1` selects an LLMGateway, OpenRouter, or generic OpenAI-compatible implementation from machine-local configuration;
- legacy LLMGateway behavior is adapted behind `LlmGatewayProviderV1` so existing task/routing semantics remain available;
- `OpenAiCompatibleProviderV1` uses standard `/chat/completions` and `/models` endpoints;
- the explicit OpenRouter profile defaults to `https://openrouter.ai/api/v1`, `OPENROUTER_API_KEY`, and `openrouter/auto`;
- provider config stores only an API-key environment-variable name, never the secret value;
- structured-output extraction/repair and SceneIntent validation remain OmniCreator-owned and provider-neutral;
- provider transport/config/API/response failures are normalized before crossing the Application Control boundary;
- sanitized LLM readiness is included in Application runtime inspection and exposed consistently through CLI, MCP and Desktop;
- CLI/MCP prefer `--llm-provider-config <path>` while retaining legacy `--llmgateway-config <path>` compatibility; the two are mutually exclusive;
- Desktop settings can select LLMGateway, OpenRouter, or generic OpenAI-compatible execution without turning provider choice into canonical project state;
- provider-free manual Content + ScenePlan takeover remains valid.

Deterministic offline acceptance covers direct OpenRouter-profile Content, structured SceneIntent and quality/reasoning execution against a local HTTP mock. Captured requests must use the OpenAI-compatible endpoint and Bearer auth while containing neither `llmgateway_task` nor any `/_llmgateway/*` call. This proves the protocol contract without claiming a live OpenRouter account/key test.

Detailed P3 behavior is documented in `docs/20-llm-providers.md`.

## P3 verification gate

P3 is not DONE / VERIFIED until all of these hold on the exact PR head and again after merge:

1. provider-neutral LLM abstraction drives Content, SceneIntent and quality/reasoning paths;
2. existing LLMGateway behavior remains green behind its adapter;
3. direct OpenRouter-profile protocol acceptance succeeds without an LLMGateway process;
4. OpenRouter requests never use `/_llmgateway/health`, `/_llmgateway/routes/explain`, or `llmgateway_task`;
5. structured-output repair and canonical SceneIntent validation remain provider-neutral;
6. provider config/readiness exposes no credential value or raw auth header and does not enter portable Data Root orchestration state;
7. CLI and MCP can use `--llm-provider-config` and inspect sanitized LLM readiness;
8. Desktop can select the provider-neutral execution mode while preserving manual fallback;
9. provider-free manual Script + ScenePlan remains usable;
10. Rust, Plugins, CLI/MCP and Desktop regressions all pass on the exact PR head;
11. guarded squash merge uses the verified exact head;
12. post-merge CI passes on the exact merge SHA before #103 is marked DONE / VERIFIED.

P4/P5 implementation must not start during the P3 verification run.
