# Phase 18 P1 — OmniCreator CLI Control Surface

Tracking: Phase 18 umbrella #99, P1 #101.

## Boundary

`omnicreator` is a transport adapter over `omnicreator-application`. It does not own workflow truth, mutate SQLite directly, create a second scheduler, or fabricate Job / Attempt / Artifact / WorkflowStep success.

```text
omnicreator CLI
      |
      v
Application Control Service
      |
      v
omnicreator-core
      |
canonical SQLite + ArtifactStore
```

Writable commands acquire the existing `WorkspaceSession` writer lease. `--read-only` opens the existing read-only application path, so mutations fail with the typed `read_only` error before core writes.

## Invocation

```text
omnicreator --data-root <path> [--read-only] [--json] <resource> <verb> [args]
```

Machine-local options:

- `--device-id <id>` overrides the writer device identity; otherwise the CLI derives one from `OMNICREATOR_DEVICE_ID`, `HOSTNAME`, or `COMPUTERNAME`.
- `--llmgateway-config <path>` enables automatic Content/Scene execution through the existing LLMGateway adapter. Credential values remain environment-backed by the referenced config and are never accepted as CLI arguments.
- `--studio-pack-catalog <path>` supplies an alternate portable Studio Pack catalog; the checked-in initial catalog is the default.

`--json` emits a compact deterministic envelope. Without it, the same envelope is pretty-printed. Human prose is intentionally not authoritative.

## Response envelope

Success:

```json
{
  "schema": "omnicreator.cli-response",
  "version": 1,
  "ok": true,
  "operation": "project.list",
  "data": {}
}
```

Failure:

```json
{
  "schema": "omnicreator.cli-response",
  "version": 1,
  "ok": false,
  "operation": "project.rename",
  "error": {
    "schema": "omnicreator.control-error",
    "version": 1,
    "code": "read_only",
    "message": "the active workspace session is read-only"
  }
}
```

Stable exit codes:

| Control error | Exit |
| --- | ---: |
| success | 0 |
| invalid_input | 2 |
| not_found | 3 |
| read_only | 4 |
| writer_conflict | 5 |
| invalid_transition | 6 |
| blocked | 7 |
| capability_unavailable | 8 |
| provider_unavailable | 9 |
| artifact_invalid | 10 |
| stale_input | 11 |
| internal | 70 |

## Command coverage

Workspace and project:

- `workspace init`, `workspace status`
- `project list`, `project show`, `project create`, `project rename`, `project delete`, `project bind-studio-pack`

Workflow and creator control:

- `workflow status`
- `workflow set-auto --project <id> --step <canonical-step> --on|--off`
- alias: `step auto ...`
- `creator state`
- `creator start|resume`
- `review list`

Manual/external takeover:

- `content provide|import`
- `scene editor|provide|import`
- `visual list|provide|asset|select-stock|approve-generated|prepare-external|provide-external`
- `voice list|provide|replace-timing|prepare-external|provide-external`

Production/recovery:

- `production status|latest|recovery|assemble|rebuild-export|export`
- `production repair-visual|repair-audio|repair-timing|repair-voice`

Sanitized runtime inspection:

- `runtime status`
- `runtime plugins` / `plugin status`
- `runtime compute` / `compute status`

Complex typed mutations accept exactly one of:

```text
--stdin
--input-file <json-file>
```

The JSON is deserialized directly into the versioned application request DTO. This avoids inventing a parallel CLI data model and lets scripts keep large content out of shell history.

## Creator Start / Resume runtime

Cross-stage sequencing remains in `CreatorRunControlServiceV1::start_or_resume_v1`.

The CLI runtime adapter may supply machine-local execution only:

- Studio Pack resolution uses the portable catalog.
- Content/Scene automatic execution uses the existing LLMGateway core adapter when `--llmgateway-config` is supplied.
- If CLI-local visual/plugin or voice/ComputeProvider execution is unavailable, the adapter returns machine-readable `capability_unavailable`. It does not pretend that a provider ran or mark the canonical step successful.
- Manual/external Phase 16 takeover remains available and a fully manual project can resume through ProductionPack/Resolve export without LLMGateway, plugins, TTS, or GPU.

## Safety and portability

The CLI does not expose raw SQL, arbitrary shell execution, a generic filesystem API, or direct SQLite mutation. Machine-local file paths are accepted only where an existing guarded import/recovery/input contract already requires a source file. Application responses retain logical/canonical identifiers and P1 regressions reject Data Root path, bearer-token marker, and API-key marker leakage.

## P1 verification requirements

P1 is DONE / VERIFIED only after:

1. workspace Rust fmt/clippy/tests pass on the exact PR head;
2. deterministic CLI full-manual acceptance reaches canonical ProductionPack + Resolve export;
3. CLI and direct Application Control Service return equivalent canonical project projection for the same state;
4. read-only mutation returns structured `read_only` and exit code 4;
5. automatic Start/Resume without configured provider returns a typed provider/capability result without Desktop or a hidden network fallback;
6. existing Plugins/Desktop regressions stay green;
7. guarded squash merge uses the verified exact PR head;
8. post-merge `main` CI passes on the exact merge SHA.

P2 MCP, P3 provider abstraction/OpenRouter, P4 agent packages, and P5 MCP Tasks/hardening are not part of P1.
