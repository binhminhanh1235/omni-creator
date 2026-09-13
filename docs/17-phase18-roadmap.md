# Phase 18 Roadmap — Agent/Application Control Plane

Authoritative GitHub tracking: umbrella #99 and slices #100 through #105.

This document records repository-level architecture/order only. GitHub issue state and verified CI evidence remain the delivery authority.

## Goal

Make Desktop, future CLI and future MCP adapters control the same canonical OmniCreator Project / WorkflowStep / Job / Attempt / ArtifactStore state without duplicating transport-specific workflow logic.

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
| P0 | #100 | Shared Application Control Service + typed operation contracts | IN PROGRESS — PR #106 foundation merged; corrective shared Start/Resume closure in progress |
| P1 | #101 | Single-binary CLI + deterministic JSON output | NOT STARTED |
| P2 | #102 | MCP server v1 over stdio | NOT STARTED |
| P3 | #103 | Provider-neutral LLM abstraction + OpenRouter direct | NOT STARTED |
| P4 | #104 | Agent skills/config/examples | NOT STARTED |
| P5 | #105 | MCP Tasks, security, E2E parity, docs and hardening | NOT STARTED |

Default sequence remains `P0 -> P1 -> P2 -> P3 -> P4 -> P5`. P0 must be DONE / VERIFIED, including post-merge main CI, before P1 starts.

## P0 architecture checkpoint

P0 introduces the transport-neutral `omnicreator-application` crate and keeps `omnicreator-core` authoritative for domain/workflow semantics.

Required P0 properties:

- writable control services can only be built from the existing single-writer `WorkspaceSession`;
- read-only inspection uses the existing read-only StateStore and application mutations fail with a typed `read_only` error;
- versioned typed requests/results/errors are serde-safe for later JSON/MCP adapters;
- project/workflow/run/manual/external/recovery/production operations delegate to canonical core paths;
- creator Start/Resume sequencing and Phase 17 automatic-execution gates live in the shared application boundary rather than Tauri commands;
- machine-local LLM/plugin/ComputeProvider execution is injected through `CreatorRunRuntimeV1`, with no Tauri type in `omnicreator-application`;
- no operation fabricates WorkflowStep success or Artifact records;
- file ingestion remains restricted to existing guarded manual/import/recovery contracts;
- machine-local Plugin/Compute runtime ownership stays outside the application crate behind small injected contracts;
- Desktop adopts workflow policy plus Start/Resume control so the abstraction has a real production consumer;
- provider-free full-manual acceptance reaches ProductionPack and Resolve/DaVinci export using canonical Jobs, Attempts and ArtifactStore verification;
- response regression tests reject unintended Data Root path and credential leakage.

Detailed contract/boundary documentation is in `docs/16-application-control-service.md`.

## P0 verification gate

P0 is not DONE / VERIFIED until all of the following are true:

1. Corrective PR exact-head Rust / Plugins / Desktop CI passes.
2. The verified exact PR head is squash-merged with an expected-head guard.
3. Merge commit/tree are re-fetched from `main`.
4. Post-merge main CI passes.
5. #100 is updated to DONE / VERIFIED with complete initial + corrective evidence.
6. #99 marks P0 complete and identifies #101 as the exact next slice.
7. Master tracking #1 records the same verified checkpoint.

No P1 implementation is part of the P0 run.
