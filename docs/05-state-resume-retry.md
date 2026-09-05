# State, DAG, Resume and Retry

## Goal

OmniCreator must resume correctly after:

- app restart
- Kaggle disconnect
- Kaggle session loss
- quota exhaustion
- individual job failure
- user editing one part of a project

Completed work must not be repeated unnecessarily.

## Canonical state

SQLite inside the selected portable Data Root is the operational source of truth for the workspace.

Remote workers are disposable.

The Data Root path itself is machine-local and may change between devices. Durable state must therefore contain logical/relative URIs rather than absolute filesystem paths.

## Workflow as DAG

A project is a dependency graph, not one long script.

Example:

```text
                    Script
                      |
             +--------+--------+
             |                 |
        Voice Prep          Scene Plan
             |                 |
            TTS          Visual Resolve
             |                 |
             |           +-----+------+
             |           |            |
             |        Stock         AI Image
             |           |            |
             +-----------+-----+------+
                             |
                          Timeline
                             |
                        Resolve Export
```

## Step states

Recommended states:

- NOT_READY
- READY
- QUEUED
- RUNNING
- SUCCEEDED
- FAILED
- RETRYABLE
- FATAL
- STALE
- SKIPPED
- CANCELLED

## Why STALE matters

If S04 narration text changes:

```text
S04 voice-prep    STALE
S04 TTS           STALE
S04 timing        STALE
dependent scenes  potentially STALE
```

Unrelated work remains valid.

The system should invalidate only downstream dependencies affected by changed inputs.

## Input hashes

Every expensive step/job should compute an input hash from all relevant deterministic inputs.

Example TTS:

```text
SHA256(
  model_version
  + voice_version
  + normalized_text
  + pace
  + emotion
  + generation_settings
)
```

If a valid artifact already exists for the same hash, return CACHE_HIT instead of using GPU.

## Job granularity

A project is not one GPU job.

Preferred:

```text
P07-S01-TTS
P07-S02-TTS
P07-S03-TTS
...
P07-SC017-IMAGE
P07-THUMB-03
```

Small independent units make retry and incremental changes cheap.

## Job vs Attempt

Separate logical work from execution attempts.

Job:

```text
P07-S04-TTS
```

Attempts:

```text
Attempt 1 -> GPU0 -> FAILED
Attempt 2 -> GPU1 -> SUCCEEDED
```

This preserves history and supports debugging.

## Suggested tables

Minimum relational model:

- projects
- steps
- jobs
- attempts
- artifacts
- dependencies

## Example Job record

```json
{
  "job_id": "job_01827",
  "project_id": "project_07",
  "step": "tts",
  "unit": "S04",
  "status": "SUCCEEDED",
  "input_hash": "abc...",
  "selected_attempt": "attempt_02"
}
```

## Example Attempt record

```json
{
  "attempt_id": "attempt_02",
  "job_id": "job_01827",
  "worker": "kaggle-gpu1",
  "started_at": "...",
  "finished_at": "...",
  "runtime_seconds": 42.7,
  "status": "SUCCEEDED",
  "error_code": null
}
```

## Error-aware retry

Do not retry every failure blindly.

Examples:

- NETWORK_TIMEOUT -> automatic retry
- WORKER_LOST -> requeue
- MODEL_LOAD_ERROR -> restart worker then retry
- CUDA_OOM -> retry with supported fallback strategy
- INVALID_VOICE -> fatal until configuration changes
- BAD_INPUT -> fatal until input changes

User-facing error messages should be simple.

Advanced details may expose traces.

## Idempotency

Workers and core must tolerate duplicate delivery.

If job ID + input hash already produced a verified artifact, a repeated request should return an already-completed result instead of regenerating.

## Artifact safety

A job is not SUCCEEDED merely because GPU inference finished.

Correct transition:

```text
GPU finished
-> artifact produced
-> artifact transferred
-> hash verified
-> artifact recorded locally
-> SQLite transaction committed
-> SUCCEEDED
```

## Reconciliation

On reconnect:

1. query remote journal/status
2. compare with local RUNNING/UNKNOWN jobs
3. download existing remote artifacts when possible
4. verify hashes
5. mark verified jobs successful
6. requeue only jobs with no valid result

Never regenerate just because the connection disappeared.

## Project status

Project-level status should be derived from step/job states.

Useful display states:

- DRAFT
- PREPARING
- NEEDS_REVIEW
- GPU_READY
- GPU_RUNNING
- GPU_PARTIAL
- READY_FOR_EDIT
- DONE

## Production Lock

Allow a project/segment to become Production Locked before GPU batching.

If the user edits a locked segment, show the exact invalidation impact:

```text
This change invalidates:
- S04 TTS
- S04 timing
- SC11-SC14 timing

Unrelated assets remain valid.
```

## Retry UX

Individual generated variants should remain selectable.

Example:

```text
S07 Voice
Take 1
Take 2
Take 3  selected
```

Retry creates a new attempt/take and does not destroy previous artifacts.


## Device handoff and cloud synchronization

Portable workspaces support **handoff**, not concurrent multi-writer collaboration.

Supported model:

```text
Machine A
  -> finish/close OmniCreator
  -> workspace reaches clean handoff revision
  -> Google Drive/copy finishes
  -> Machine B selects the same Data Root
  -> validate
  -> resume
```

Do not allow two devices to actively mutate the same synchronized workspace at the same time.

### Workspace identity

`.omnicreator/workspace.json` should contain durable identity/version metadata such as:

```json
{
  "workspace_id": "ws_...",
  "schema_version": 1,
  "revision": 184,
  "last_clean_shutdown": true,
  "last_writer_device": "device_...",
  "updated_at": "..."
}
```

Machine-specific absolute Data Root paths are not stored here.

### Single-writer protection

Use two layers:

1. OS/filesystem lock for same-filesystem concurrent access
2. a synchronized best-effort lease/heartbeat record containing device ID and session information

Cloud file synchronization cannot provide a trustworthy distributed lock. If another recent writer is detected, OmniCreator should warn and default to read-only/recovery instead of blindly opening as writer.

### Clean handoff snapshot

On graceful close or explicit **Prepare for Device Handoff**:

1. stop accepting new mutating jobs
2. finish/commit current local transactions
3. verify artifact writes that are already marked complete
4. create a consistent SQLite backup/snapshot
5. hash the snapshot
6. write a handoff manifest referencing the revision + snapshot hash
7. atomically mark the workspace clean

Keep a small rotating set of clean state snapshots.

On a new machine:

1. validate workspace manifest
2. verify the referenced clean snapshot/hash
3. run SQLite integrity validation when appropriate
4. recover from the clean snapshot if the working DB is damaged/incomplete
5. reconcile artifact hashes
6. resume the DAG

This gives the workspace a recovery anchor even if a sync service captured files at awkward moments.

### SQLite sync policy

A synchronized folder is not a multi-machine database service.

For syncable workspaces:

- prefer a journal configuration that does not depend on portable WAL/SHM files as durable state
- close/checkpoint cleanly before device handoff
- never depend on copying an actively-mutating SQLite file for correctness
- keep verified clean backups
- require a single active writer

### Artifact sync safety

Generated/downloaded artifacts should be written to temporary names and atomically promoted to final workspace paths after completion.

An artifact is valid only when:

- expected file exists
- size is plausible
- recorded hash matches

If Google Drive has not fully synchronized a file yet, keep the dependent step WAITING_FOR_ARTIFACT/NOT_READY rather than treating it as failed or regenerating immediately.

### Resume on a second machine

After binding to an existing Data Root, OmniCreator should reconstruct the same project board and DAG states:

- DONE stays DONE
- GPU_READY stays GPU_READY
- successful TTS takes remain selectable
- retry history remains available
- unfinished jobs are re-evaluated against artifacts/cache
- remote RUNNING jobs from a dead old session become reconcilable/retryable

No completed job should be repeated solely because the physical machine changed.

## Production package export attempts

Phase 9 P2 uses the same `Job -> Attempt -> Artifact` state machine for local production-package export as plugin/GPU work.

A production export Job is keyed by a deterministic execution input hash. One Attempt stages all required components before promotion. `ArtifactStore` can promote multiple verified outputs for the same Attempt, and `StateStore` records all component Artifacts plus the selected production-pack artifact in one SQLite transaction before marking the Attempt and Job `SUCCEEDED`.

This preserves the core safety rule: partial output is not success.

If local rendering, physical verification or promotion fails, the export Attempt transitions through `LOCAL_EXPORT_ERROR`, which is retryable. A restart still uses the normal RUNNING-to-RETRYABLE reconciliation path. There is no second export-state model.

### Export cache and Data Root binding

The portable semantic hash covers normalized `ProductionPackV1`, exporter/profile/layout versions, canonical artifact facts and the provenance fields that affect the source report. It excludes timestamps and random UUIDs.

FCPXML is path-bearing, so package execution additionally incorporates an opaque hash of the current Data Root binding. The absolute path itself is never persisted in canonical state. Moving/rebinding the Data Root therefore invalidates the path-bearing package variant while leaving the portable semantic hash and canonical media references untouched.

A cache hit is accepted only when the complete package belongs to a SUCCEEDED producer Job and every cached Artifact still passes physical hash verification.
