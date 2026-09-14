# Phase 18 Roadmap — Agent/Application Control Plane

Authoritative GitHub tracking: umbrella #99 and slices #100 through #105.

This document records repository-level architecture/order only. GitHub issue state and verified CI evidence remain the delivery authority.

## Goal

Make Desktop, CLI and agent-facing MCP/CLI adapters control the same canonical OmniCreator Project / WorkflowStep / Job / Attempt / ArtifactStore state without duplicating transport-specific workflow logic.

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
| P3 | #103 | Provider-neutral LLM abstraction + OpenRouter direct | DONE / VERIFIED — PR #110; post-merge CI #563 PASS |
| P4 | #104 | Agent skills/config/examples | IN PROGRESS — PR #111 / `feat/phase18-p4-agent-harness-packages` |
| P5 | #105 | MCP Tasks, security, E2E parity, docs and hardening | NOT STARTED |

Default sequence remains `P0 -> P1 -> P2 -> P3 -> P4 -> P5`.

## P0 verified checkpoint

P0 introduced the transport-neutral `omnicreator-application` crate while keeping `omnicreator-core` authoritative for domain/workflow semantics.

Verified properties:

- writable control services can only be built from the existing single-writer `WorkspaceSession`;
- read-only inspection uses the existing read-only StateStore and application mutations fail with typed `read_only`;
- versioned typed request/result/error contracts are serde-safe for later JSON/MCP adapters;
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

- final exact head `4a07af1aba964823dc48f9795ec671a66d96bf0f`
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

## P3 verified checkpoint

P3 made LLMGateway one supported creator-intelligence provider instead of the only transport and added direct OpenRouter through an OpenAI-compatible adapter.

Verified architecture:

- `LlmProviderV1` defines provider-neutral chat/model/readiness behavior below shared creator execution;
- `ConfiguredLlmProviderV1` selects LLMGateway, OpenRouter, or generic OpenAI-compatible execution from machine-local configuration;
- legacy LLMGateway behavior remains behind `LlmGatewayProviderV1`;
- `OpenAiCompatibleProviderV1` uses standard `/chat/completions` and `/models` endpoints;
- the OpenRouter profile defaults to `https://openrouter.ai/api/v1`, `OPENROUTER_API_KEY`, and `openrouter/auto`;
- provider config stores only an API-key environment-variable name, never the secret value;
- structured-output extraction/repair, SceneIntent validation, and quality/reasoning remain OmniCreator-owned and provider-neutral;
- provider transport/config/API/response failures are normalized before crossing the Application Control boundary;
- sanitized LLM readiness is exposed consistently through CLI, MCP and Desktop;
- CLI/MCP prefer `--llm-provider-config <path>` while retaining legacy `--llmgateway-config <path>` compatibility;
- Desktop settings select LLMGateway, OpenRouter, or generic OpenAI-compatible execution without turning provider choice into canonical project state;
- provider-free manual Content + ScenePlan takeover remains valid.

P3 final evidence:

- PR #110 final exact head `81e66863ef42188004631fa1566ef07204b699c2`
- exact-head tree `d0ba79745eec60327313bd821c6d4511e99d1bc6`
- exact-head CI #562 / run `34814316035`: PASS across Rust / Plugins / Desktop
- guarded squash merge `8490e67e48e985a497473f3c3c9ea39ea158b9dd`
- merge tree `d0ba79745eec60327313bd821c6d4511e99d1bc6`
- post-merge CI #563 / run `34818696412`: PASS across Rust / Plugins / Desktop

Deterministic offline acceptance covers direct OpenRouter-profile Content, structured SceneIntent and quality/reasoning execution against a local HTTP mock. It proves the protocol contract without claiming a live OpenRouter account/key test.

Detailed P3 behavior is documented in `docs/20-llm-providers.md`.

## P4 architecture checkpoint

P4 packages the already-verified CLI/MCP control plane for mainstream agent harnesses without introducing vendor-owned workflow truth.

Current implementation:

- Codex and Google Antigravity share the repository skill at `.agents/skills/omnicreator/SKILL.md`;
- Claude Code uses a byte-identical copy at `.claude/skills/omnicreator/SKILL.md`;
- checked-in Codex, Claude Code and Antigravity examples all launch the same local stdio process: `omnicreator --data-root <path> mcp serve`;
- direct OpenRouter and LLMGateway provider examples use the P3 machine-local config schemas and contain only environment-variable names, never credential values;
- the skill requires inspect-before-mutate, returned canonical IDs, re-inspection after mutation, truthful blocker handling, resume/recovery over state recreation, and canonical manual/external takeover;
- the skill forbids direct SQLite/Data Root/ArtifactStore state mutation and fabricated provider/plugin/TTS/GPU/ComputeProvider success;
- English and Vietnamese setup guides plus deterministic CLI examples cover all-auto, mixed auto/manual, fully manual, and recovery workflows;
- `agent-harness/fixtures/blocked-workflow-recovery.json` records a provider-free recovery transcript contract that preserves `provider_unavailable`, turns a step AUTO OFF, satisfies it manually, and re-inspects before continuing;
- `crates/omnicreator-cli/tests/agent_harnesses.rs` byte-compares skill copies, validates provider config through the real core schema, checks secret-like markers and fixture sequencing, then launches the real `omnicreator ... mcp serve` child with the official rmcp client to prove every fixture tool exists on the actual MCP server;
- the pre-existing MCP E2E suite remains the executable proof that the provider-free fully manual flow can reach ProductionPack/export.

Detailed P4 setup and safety guidance is in `docs/21-agent-harnesses.md` and `agent-harness/README.md`.

## P4 verification gate

P4 is not DONE / VERIFIED until all of these hold on the exact PR head and again after merge:

1. Codex, Claude Code, and Antigravity each have a documented local MCP configuration path to the real single-binary stdio server;
2. repository skills encode the inspect/mutate/re-inspect safety contract and cannot drift between `.agents` and `.claude` copies;
3. OpenRouter and LLMGateway examples validate through the P3 provider schema without containing secret values;
4. all-auto, mixed auto/manual, fully manual, AUTO OFF, Review Center/recovery, and provider selection behavior are documented;
5. the blocked-workflow fixture preserves truthful `provider_unavailable` and manual recovery semantics;
6. a real official rmcp client launches the actual binary and proves every fixture tool name exists;
7. existing provider-free MCP full-manual ProductionPack/export E2E remains green;
8. no vendor-specific workflow database, remote MCP HTTP, arbitrary shell control tool, or canonical-state bypass is introduced;
9. Rust, Plugins, CLI/MCP and Desktop regressions pass on the exact PR head;
10. guarded squash merge uses the verified exact head;
11. post-merge CI passes on the exact merge SHA before #104 is marked DONE / VERIFIED.

P5 MCP Tasks/security/final hardening remains out of scope until P4 is closed and verified.
