# Review Center Universal Recovery

Phase 16 P6 turns Review Center into the creator-facing recovery surface for blocked, failed, unavailable, manual-required and invalid-result states.

The architecture remains deliberately narrow:

```text
canonical Project / WorkflowStep / Job / Attempt / Artifact
+ Studio Pack capability/readiness
+ Production Recovery artifact verification
                    |
                    v
             Review Center
          projection + controller
                    |
                    v
        existing canonical controllers
```

Review Center owns no durable workflow truth. It has no recovery database, scheduler or browser-persisted state. Restart and Data Root move/rebind therefore reconstruct the same recovery view from canonical state and logical artifact identities.

## Recovery actions

The provider-neutral action vocabulary is:

- **Retry**: requeue an actual retryable canonical Job through the existing retry controller.
- **Configure**: navigate to the owning LLM/plugin/runtime setup surface when configuration is required.
- **Choose Alternative**: available only when product semantics require an explicit choice and at least two ready candidates satisfy the exact required capability.
- **Provide Manually**: navigate to the Phase 16 manual takeover flow for Script/ScenePlan, Visual, Audio/Timing or an applicable external handoff.
- **Replace Result**: repair/relink a missing or invalid selected artifact through the canonical replacement and Production Recovery paths.
- **Details**: inspect stage/item identity, blocker classification, reason, result health, canonical source and currently valid actions.

The UI recommends one primary action and keeps the rest secondary. It does not show every action on every card.

Ordered Studio Pack targets remain deterministic fallback order. A ready generic generated-still provider is not presented as an alternative to `stick_figure_visual`; capability semantics stay exact. P6 also does not pretend the current deterministic fallback list is an explicit user-selection menu.

## Canonical reuse

P6 dispatches into existing Phase 16 paths instead of reimplementing them:

- P1 manual Script / ScenePlan ingestion;
- P2 manual image/video ingestion;
- P3 manual WAV and timing ingestion;
- P4 external generated-image / voice handoff;
- P5 ProductionPack artifact inspection, repair/relink, rebuild and Resolve export.

Validation, provenance, Job/Attempt history, dependency invalidation, ArtifactStore ingestion and physical hash verification remain in core.

## Invalid selected results

Production Recovery is queried from canonical state after Content + ScenePlan exist. Missing or hash-invalid selected visual/audio/timing artifacts become `Replace Result` cards.

Replacement does not rewrite history. The replacement path creates/verifies the new artifact, supersedes or stales the affected canonical producer result as defined by the Phase 16 manual/recovery contracts, and invalidates only the dependency cone owned by those contracts. Unrelated verified work remains reusable.

## Read-only workspaces

Read-only workspaces may:

- inspect Review Center;
- open Details;
- inspect artifact/recovery health;
- inspect non-secret canonical context.

Mutation actions are rendered disabled. Backend writable guards remain authoritative, so UI suppression is not the security/safety boundary.

## Safety and privacy

Recovery action/detail payloads intentionally exclude:

- API keys and auth tokens;
- cookies;
- provider-private request payloads;
- browser/session transport internals;
- absolute machine paths.

Logical URIs and canonical IDs are safe portable identifiers and may be shown for repair/relink diagnostics.

## Fully manual acceptance

The critical Phase 16 invariant is verified by `creator_phase16_fully_manual.rs`:

```text
Script
  -> ScenePlan
  -> Visual
  -> Audio
  -> Timing
  -> ProductionPack
  -> DaVinci / Resolve export
```

No LLM, stock provider, generated-image provider, voice provider, GPU or other external service is required by that scenario. Automatic execution remains an accelerator, never the only continuation path.

## Regression gates

P6 adds:

- core unit tests for action applicability, exact-capability alternatives, deterministic fallback semantics, read-only behavior, invalid-result replacement, retry classification and sanitized payloads;
- a deterministic fully manual end-to-end integration test;
- Desktop Review Center P6 syntax + smoke regression in CI;
- existing Phase 15 golden path, manual/external handoff, Production Recovery, plugin and desktop suites remain part of the workspace CI gate.
