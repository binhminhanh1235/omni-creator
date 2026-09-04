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

Local SQLite is the source of truth.

Remote workers are disposable.

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
