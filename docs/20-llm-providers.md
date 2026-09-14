# Phase 18 P3 — Provider-neutral LLM runtime

Tracking: Phase 18 umbrella #99, P3 #103.

## Boundary

OmniCreator creator-intelligence code depends on a provider-neutral LLM contract. LLMGateway is one provider implementation, not workflow truth and not a required transport.

```text
Desktop / CLI / MCP
        |
Application Control / Creator Run Control
        |
ConfiguredLlmProviderV1
        |
   +----+--------------------+
   |                         |
LlmGatewayProviderV1   OpenAiCompatibleProviderV1
                              |
                          OpenRouter profile
```

Provider selection is machine-local execution configuration. Canonical Project / WorkflowStep / Job / Attempt / ArtifactStore state remains provider-neutral and portable.

Manual Content and ScenePlan takeover remains valid with no LLM provider configured.

## Provider configuration

The preferred machine-local config is `omnicreator.llm-provider-config` v1. Secret values are never stored in this file; the config stores only the environment-variable name that contains the credential.

Example OpenRouter profile:

```json
{
  "schema": "omnicreator.llm-provider-config",
  "version": 1,
  "provider": {
    "kind": "open_router",
    "config": {
      "schema": "omnicreator.openai-compatible-config",
      "version": 1,
      "provider_id": "openrouter",
      "provider_kind": "open_router",
      "base_url": "https://openrouter.ai/api/v1",
      "api_key_env": "OPENROUTER_API_KEY",
      "default_model": "openrouter/auto",
      "timeout_seconds": 60,
      "app_title": "OmniCreator"
    }
  }
}
```

The credential is supplied separately to the process environment:

```text
OPENROUTER_API_KEY=<secret>
```

Do not place the key itself in Data Root, Project IR, SceneIntent, CLI arguments, MCP payloads, Desktop projections, or provider config files.

`OpenAiCompatibleProviderV1` also supports an explicit `open_ai_compatible` profile with a configurable base URL, provider ID, model and credential environment-variable name.

Legacy `--llmgateway-config <path>` remains supported. New callers should prefer `--llm-provider-config <path>`. The two flags are mutually exclusive.

## OpenRouter protocol behavior

The OpenRouter profile uses the OpenAI-compatible API surface:

- chat: `<base_url>/chat/completions`
- model discovery: `<base_url>/models`
- authentication: `Authorization: Bearer <credential from api_key_env>`

OpenRouter execution never calls LLMGateway-only endpoints such as `/_llmgateway/health` or `/_llmgateway/routes/explain`, and OpenAI-compatible request bodies never contain `llmgateway_task`.

Provider/model identity is execution configuration and sanitized runtime provenance. It is not portable workflow orchestration truth.

## Creator-intelligence paths

The same provider-neutral boundary drives:

- Content/script generation;
- structured SceneIntent generation;
- quality/reasoning structured evaluation.

Structured-output extraction, bounded repair and canonical validation remain OmniCreator-owned. SceneIntent still passes the existing schema and semantic validators regardless of provider.

## Runtime inspection

Application runtime inspection exposes a sanitized LLM snapshot containing only non-secret execution metadata such as:

- provider ID and kind;
- readiness state;
- base URL;
- API-key environment-variable name;
- default model;
- whether the credential environment variable is present;
- whether model discovery is supported;
- normalized reason code when setup is incomplete.

The credential value and raw authorization header are never returned.

CLI examples:

```text
omnicreator --data-root <path> --llm-provider-config <path> runtime llm
omnicreator --data-root <path> --llm-provider-config <path> runtime status
omnicreator --data-root <path> --llm-provider-config <path> creator start --project <id> --topic "..."
```

MCP uses the same global provider config:

```text
omnicreator --data-root <path> --llm-provider-config <path> mcp serve
```

The MCP `runtime_status` tool accepts the LLM inspection kind in addition to the existing all/plugins/compute views.

## Desktop

Desktop provider settings use the same machine-local provider-neutral configuration model. The UI can select:

- LLMGateway;
- OpenRouter;
- generic OpenAI-compatible provider.

Desktop readiness/status commands are provider-neutral. Selecting OpenRouter does not route through LLMGateway commands or LLMGateway-only health endpoints.

## Errors

Provider failures are normalized into provider-neutral typed errors before they cross the application boundary. Important categories include invalid provider configuration, missing credential environment variable, provider transport/API failure and invalid provider response.

CLI/MCP/Desktop projections must remain sanitized and must not echo provider response bodies when they may contain sensitive/debug payloads.

## Verification

P3 includes deterministic offline protocol acceptance using a local HTTP mock. The acceptance proves that a direct OpenRouter-profile provider can execute:

1. Content/script generation;
2. structured SceneIntent generation with canonical validation;
3. quality/reasoning structured evaluation.

The captured requests must use `/api/v1/chat/completions`, Bearer authentication and the configured OpenRouter model, while containing neither `llmgateway_task` nor any `/_llmgateway/*` path.

This is protocol-contract verification, not a claim that a live OpenRouter account/key was exercised. A live-provider test may be performed separately when an explicit credential is available, but it is not required for deterministic CI.

Existing LLMGateway behavior, provider-free manual takeover, creator workflow determinism and CLI/MCP/Desktop regressions remain required gates for P3 closure.
