# Kaggle Burst Compute Strategy

## Objective

Kaggle free GPU time is limited. Treat every active Kaggle session as a **burst compute window**.

The goal is not to keep Kaggle involved throughout project creation.

The goal is:

> Prepare all non-GPU work first, then keep the GPUs busy with a pre-built queue.

## Core rule

**Never turn on Kaggle and then start preparing jobs.**

Prepare projects first:

```text
Project 01 -> GPU READY
Project 02 -> GPU READY
Project 03 -> GPU READY
...
Project 10 -> GPU READY

then connect Kaggle once
```

## What belongs outside Kaggle

Do locally or through external APIs:

- research
- script writing
- biblical/content QA
- segmentation
- voice-direction preparation
- pronunciation rules
- scene planning
- Pexels search
- Pexels preview ranking
- stock asset download
- generated-image prompt preparation
- thumbnail planning
- subtitles from returned timing data
- timeline generation
- FCPXML
- file organization
- metadata/provenance
- hashing/cache lookup

## What belongs on Kaggle

Only tasks that materially benefit from GPU:

- OmniVoice generation
- generated images
- optional heavy VLM
- optional heavy audio enhancement/alignment
- future GPU-capable plugins

## GPU Ready contract

A job may enter the GPU queue only if:

- all dependencies succeeded
- input is resolved and immutable
- plugin/provider is known
- model version is known
- settings are known
- output destination is known
- cache lookup has already failed
- human approval is complete if required

Otherwise the job remains NOT_READY.

## Batch planning

Provide a GPU Workbench:

```text
Ready projects:          10

OmniVoice:
  137 segments

Images:
  43 scenes

Thumbnail backgrounds:
  18

Estimated workload:
  3h 40m
```

The app can recommend preparing more projects before starting Kaggle when the queue is too small.

## Group by model

Avoid repeated model loading/unloading.

Preferred:

```text
load OmniVoice once
  -> process many TTS jobs
unload when useful

load image model once
  -> process image jobs
```

Use model_group / affinity metadata.

## T4 x2 scheduling

Do not treat 2 x T4 as one unified 32 GB GPU.

Default strategies:

### TTS-heavy batch

```text
GPU0 OmniVoice A
GPU1 OmniVoice B

distribute independent segments
```

### Mixed batch

```text
GPU0 OmniVoice
GPU1 Image Generation
```

The scheduler chooses based on queue shape and plugin capability.

## Utilization metric

Optimize:

```text
useful GPU compute / Kaggle wall-clock time
```

not simply job count.

Idle time caused by manual decisions, search, download or prompt preparation should approach zero.

## Runtime packaging

Kaggle startup should avoid repeated environment setup.

Maintain a versioned runtime dataset/cache containing as much reusable material as practical:

- wheels
- model files
- tokenizers
- OmniVoice runtime
- configs
- bootstrap script

Target startup:

```text
session starts
-> attach runtime
-> detect GPUs
-> start worker
-> READY
```

## Immutable model versions

Jobs should record explicit versions:

```text
model: omnivoice-v3.2
voice: warm-narrator-v4
```

Changing model versions changes input hashes and prevents incorrect cache reuse.

## Immediate artifact sync

Do not wait until the batch ends to download one large archive.

Preferred:

```text
generate artifact
-> hash
-> transfer to Mac
-> verify hash
-> commit local state
-> next job
```

This protects completed work when:

- session ends
- tunnel disconnects
- quota expires
- Kaggle crashes

## Local + remote journal

Local SQLite is canonical.

Kaggle should maintain a lightweight session journal such as JSONL:

```json
{"job":"1827","status":"done","artifact":"..."}
{"job":"1828","status":"failed","error":"..."}
```

On reconnect, OmniCreator reconciles local and remote states.

## Weekly GPU budget

Track:

- estimated weekly allowance
- used session time
- remaining time
- ready workload
- historical runtime per model/plugin

Estimates should improve from actual job history.

A simple moving average/EMA is sufficient for v1.

## Provider-neutral desktop bridge

The desktop control plane talks to a disposable GPU worker through the provider-neutral ComputeProvider HTTP bridge. Kaggle is one possible host, but no Kaggle-specific field is allowed in core scheduling or execution contracts.

Machine-local connection configuration contains only:

- provider ID
- worker base URL
- optional bearer-token environment-variable name
- timeout

The bearer token value itself is never written to the portable Data Root. Runtime transfer staging belongs in the app cache, not project storage.

Protocol v1 uses JSON requests over these endpoints:

```text
POST /v1/compute/connect
POST /v1/compute/disconnect
POST /v1/compute/heartbeat
POST /v1/compute/capabilities
POST /v1/compute/dispatch
POST /v1/compute/journal
POST /v1/compute/artifact
```

`dispatch` is queue admission for an immutable logical job attempt pinned to the reviewed provider/session/device selection. The desktop submits assignments in deterministic Burst wave/order. A worker must serialize its own per-device queue while allowing independent devices to make progress concurrently. It must never reinterpret two T4 devices as pooled VRAM.

The journal remains append-only session evidence. The desktop polls it after Burst start, transfers ready artifacts immediately, verifies hashes, commits them locally, and derives running/completed/remaining/retryable UI from canonical SQLite jobs and attempts.

A provider reconnect or capability change invalidates the reviewed schedule hash and requires Prepare GPU Batch again before new dispatch.

## Burst Mode

Kaggle default mode should be non-interactive:

- maximum queue throughput
- automatic retry for retryable failures
- continuous artifact sync
- no human prompts during execution
- no unrelated planning tasks

Human review should happen before expensive rendering whenever possible.
