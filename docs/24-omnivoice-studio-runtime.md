# OmniVoiceStudio runtime endpoint

Tracking: Phase 19 #113, P5 #119, runtime endpoint slice #126.

## Why this exists

A public OmniVoiceStudio process can be exposed through a Cloudflare quick tunnel. The generated `*.trycloudflare.com` hostname changes when the Studio/tunnel is restarted, so that hostname must not be compiled into OmniCreator or persisted as portable project truth.

OmniCreator therefore treats the OmniVoiceStudio address as **machine-local runtime configuration**.

## Change the domain while OmniCreator is running

Open the **OmniVoiceStudio** button in the Desktop top bar. Paste any one of the URLs printed by OmniVoiceStudio:

```text
https://example.trycloudflare.com/ui
https://example.trycloudflare.com/api/v1
https://example.trycloudflare.com/mcp
```

You may also paste the root URL:

```text
https://example.trycloudflare.com
```

Choose **Save & connect**. No OmniCreator restart is required. The saved value is normalized to the root domain and the current runtime endpoints become:

```text
Studio UI:  <base>/ui
REST API:   <base>/api/v1
MCP:        <base>/mcp
Health:     <base>/health
Capabilities: <base>/api/v1/capabilities
Audio generation: <base>/api/v1/audio/generate
```

The next status refresh/runtime operation reads the current machine config, so replacing the tunnel domain takes effect immediately.

## Authentication

Public OmniVoiceStudio REST/MCP can require a bearer token. OmniCreator defaults to the same environment variable used by OmniVoiceStudio:

```text
OMNIVOICE_API_TOKEN
```

Only the **environment-variable name** is saved. The token value is read from the process environment when the runtime is probed and is never written into the OmniCreator config, Data Root, project artifacts, external descriptors, logs, or handoff state.

The bearer-token environment variable field can be left empty when the target Studio intentionally allows unauthenticated machine API access.

## Runtime verification

`Save & connect` and `Refresh` verify the configured target instead of trusting the hostname blindly:

1. `GET <base>/health`
2. `GET <base>/api/v1/capabilities`
3. verify that standalone audio generation is advertised
4. surface MCP availability and bearer-auth failures truthfully

The panel reports `READY`, `NEEDS API TOKEN`, `OFFLINE`, or `DEGRADED` rather than converting a failed tunnel/provider probe into success.

## Portability boundary

The machine config is stored under the Tauri application config directory as `omnivoice-studio.json`. It is deliberately outside the portable OmniCreator Data Root.

This preserves the Phase 19 boundary:

- Project/Data Root owns canonical workflow and artifact truth.
- OmniVoiceStudio tunnel URL, credentials and provider capacity are machine-local runtime facts.
- Changing a tunnel domain never rewrites canonical project history.
- External voice results still become canonical only through the existing verified OmniCreator result-ingress path.

## Accepted URL forms

The runtime normalizer accepts the root URL and the printed `/ui`, `/api/v1`, `/mcp`, `/health`, or `/api/v1/capabilities` surfaces. It rejects:

- non-HTTP(S) URLs;
- embedded `user:password@host` credentials;
- query strings or fragments;
- arbitrary extra URL paths.

This keeps the runtime locator deterministic and avoids accidentally persisting secrets in the endpoint string.
