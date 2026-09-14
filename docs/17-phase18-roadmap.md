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
| P1 | #101 | Single-binary CLI + deterministic JSON output | IN PROGRESS |
| P2 | #102 | MCP server v1 over stdio | NOT STARTED |
| P3 | #103 | Provider-neutral LLM abstraction + OpenRouter direct | NOT STARTED |
| P4 | #104 | Agent skills/config/examples | NOT STARTED |
| P5 | #105 | MCP Tasks, security, E2E parity, docs and hardening | NOT STARTED |

Default sequence remains `P0 -> P1 -> P2 -> P3 -> P4 -> P5`. P2 must not start until P1 is DONE / VERIFIED unless the authoritative tracking explicitly changes the order.

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

## P1 architecture checkpoint

P1 adds one Rust CLI binary, `omnicreator`, as a transport adapter over the P0 Application Control Service.

Required P1 properties:

- Data Root is selected explicitly with `--data-root`; writable commands acquire the existing `WorkspaceSession` writer lease;
- `--read-only` uses `ApplicationControlService::for_read_only`, so read-only is enforced at the application layer rather than by presentation logic;
- CLI resource/verb commands invoke P0 typed operations instead of SQLite, StateStore internals, or duplicated workflow orchestration;
- `creator start|resume` delegates to `CreatorRunControlServiceV1::start_or_resume_v1`;
- JSON output uses one versioned deterministic envelope and preserves machine-readable P0 error categories;
- stable process exit codes map directly from typed control errors;
- complex content/ScenePlan/visual/voice/recovery requests may arrive through `--stdin` or `--input-file`, avoiding a second request schema and reducing secret/content exposure in shell history;
- LLMGateway credentials remain environment-backed by the machine-local config; credential values are never accepted as CLI flags or persisted in project state;
- CLI runtime adapters may report `provider_unavailable` / `capability_unavailable` when a machine-local provider/plugin/ComputeProvider is not configured; they must never fabricate READY/SUCCEEDED state;
- Phase 16 manual/external takeover remains sufficient for a provider-free full-manual CLI flow through ProductionPack and Resolve export;
- runtime inspection is sanitized and truthful; an unconfigured CLI-local plugin/compute runtime is reported as unconfigured, not healthy;
- no MCP protocol, OpenRouter/provider migration, agent package, or MCP Tasks implementation belongs to P1.

Command and JSON details are documented in `docs/18-cli-control.md`.

## P1 verification gate

P1 is not DONE / VERIFIED until all of the following are true:

1. Exact PR-head workspace Format / Clippy / Tests pass, including CLI integration tests.
2. Existing Plugins and Desktop regression jobs remain green.
3. CLI full-manual acceptance reaches canonical ProductionPack + Resolve/DaVinci export.
4. CLI project projection matches direct Application Control Service projection for the same canonical state.
5. Read-only mutation returns typed `read_only` without persisting a change.
6. Automatic Creator Start/Resume can report provider/capability blockers without requiring Desktop or silently falling back to fake success.
7. CLI JSON regressions reject Data Root/credential marker leakage.
8. The verified exact PR head is squash-merged with an expected-head guard.
9. Merge commit/tree are re-fetched from `main` and post-merge CI passes.
10. #101, #99 and master tracking #1 are updated with exact evidence before P1 is marked DONE / VERIFIED.

P2 implementation is not part of the P1 run.
