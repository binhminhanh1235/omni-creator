# System Architecture

## Architectural style

OmniCreator uses a **local control plane + pluggable compute workers** architecture.

The local machine owns project state and orchestration. Remote GPU environments are temporary workers.

```text
                  OmniCreator Desktop
                    Rust + Tauri
                         |
          +--------------+---------------+
          |              |               |
      Project Core   Plugin Runtime   Workflow/DAG
          |              |               |
          +--------------+---------------+
                         |
          +--------------+---------------------+
          |              |                     |
      LLMGateway     Local Providers       ComputeProvider
          |              |                     |
          |           Pexels                Kaggle
          |           Assets                T4 x2
          |                                     |
          |                              Python AI Worker
          |                              +-------------+
          |                              | OmniVoice   |
          |                              | Images      |
          |                              | optional VLM|
          |                              +-------------+
          |
       external APIs

                         |
                  Local Artifact Store
                         |
                 Timeline / SRT / FCPXML
                         |
                  DaVinci Resolve
```

## Local responsibilities

The MacBook should handle lightweight, stateful or editor-adjacent work:

- Tauri UI
- project management
- SQLite database
- workflow orchestration
- dependency tracking
- LLMGateway client
- Pexels search
- candidate preview display
- selected asset download
- file organization
- metadata extraction
- hashes and cache lookup
- subtitle assembly
- timeline calculation
- FCPXML/export generation
- thumbnail compositing
- artifact/source tracking

These tasks do not justify consuming Kaggle GPU quota.

## Remote GPU responsibilities

Kaggle should only receive jobs that benefit materially from GPU compute:

- OmniVoice TTS
- generated images
- optional heavy vision models
- optional heavy audio models
- other future GPU plugin tasks

Kaggle must not be the source of truth for project state.

## ComputeProvider abstraction

Kaggle is the first remote ComputeProvider, not a hard-coded dependency.

Future providers can include:

- local GPU
- Colab
- RunPod
- Vast.ai
- cloud GPU
- other remote workers

A plugin declares resource requirements. The scheduler chooses an available ComputeProvider.

Example capability requirement:

```yaml
resources:
  gpu: required
  min_vram_mb: 12000
  model_group: omnivoice-v3
  parallelizable: true
```

## Why not Kaggle-only

A full Kaggle-hosted OmniCreator would couple:

- persistent project state
- media assets
- temporary GPU sessions
- DaVinci-local editing
- download/upload cycles

That creates friction and makes resume/retry brittle.

The local app should survive the complete loss of a Kaggle session without losing project truth.

## Why not local-only

The M2 Pro is suitable for the control plane and DaVinci workflow, but 16 GB unified memory should not be consumed by large AI models while editing.

"Can run locally" is not the same as "should run locally."

Heavy AI remains remote/API by default.

## Language strategy

### Rust

Recommended for:

- Tauri backend
- scheduler
- workflow engine
- plugin runtime
- HTTP clients
- downloads
- SQLite access
- artifact management
- hashing
- process supervision
- timeline exporters

### Python

Recommended for:

- OmniVoice
- PyTorch
- Transformers
- Diffusers
- model-specific GPU code
- Kaggle AI worker

### Boundary

Prefer process/API boundaries rather than embedding Python directly in Rust for v1.

```text
Rust Core
   |
 JSON / HTTP / WebSocket
   |
Python Worker
```

This keeps failures isolated and makes local/remote deployment use the same protocol.

## Data locality rule

Avoid moving large media through Kaggle unless GPU processing truly requires it.

Example stock workflow:

```text
Pexels metadata
   |
preview frames
   |
ranking
   |
select asset
   |
download directly to Mac
   |
DaVinci
```

No remote transcode loop should exist in the default workflow.

## Performance philosophy

Primary performance wins should come from:

1. avoiding unnecessary work
2. caching
3. incremental invalidation
4. preparing work before GPU sessions
5. grouping jobs by model
6. parallelizing independent units
7. immediate artifact sync
8. keeping expensive resources busy only with useful jobs

Language-level optimization is secondary to workflow-level optimization.


## Portable Data Root

All durable OmniCreator creator data lives under one user-selected **Data Root**.

Example:

```text
OmniCreatorData/
├── .omnicreator/
│   ├── workspace.json
│   ├── state/
│   │   └── omnicreator.sqlite
│   ├── backups/
│   └── handoff/
├── projects/
├── library/
│   └── assets/
├── studio-packs/
├── channel-profiles/
├── plugin-data/
├── exports/
└── metadata/
```

The Data Root may be:

- a normal local folder
- an external SSD folder
- a folder synchronized by Google Drive or another file-sync service

### Machine-local pointer

The application keeps only a tiny machine-local setting outside the Data Root:

```text
data_root = /Users/.../OmniCreatorData
device_id = ...
```

On another machine the path may be completely different. The user chooses **Use Existing Data Folder**, and OmniCreator binds that machine to the workspace.

### No absolute paths in durable project state

Project/database records must not persist machine-specific paths such as:

```text
/Users/alice/Google Drive/OmniCreatorData/projects/P001/audio/S01.wav
```

Use logical references instead:

```text
workspace://projects/P001/audio/S01.wav
project://audio/S01.wav
artifact://AUDIO_221
```

The path resolver combines the current machine's Data Root with the logical URI at runtime.

This is required for:

- copying the folder to another Mac
- moving the folder
- Google Drive synchronization
- external SSD workflows
- future Windows/Linux support

### Portable vs machine-specific data

The Data Root contains everything required to continue creator work:

- projects and scripts
- SQLite state
- selected/downloaded/generated assets
- narration/takes
- provenance/license metadata
- channel profiles
- Studio Packs
- plugin configuration
- plugin version requirements
- timeline/export source data

Do not put these machine-specific items in the portable root by default:

- API secrets/passwords
- macOS Keychain data
- disposable temp/cache
- platform-specific plugin binaries
- Python/AI runtimes
- DaVinci executable path
- device-specific GPU settings

The workspace may store credential references, but actual secrets remain in the OS credential store.

### Plugin portability

Persist a plugin lock/config description in the Data Root.

On another machine OmniCreator should:

1. read required plugin IDs and versions
2. compare against installed plugins
3. automatically use bundled plugins when available
4. show one simple **Install Missing Plugins** action when required
5. preserve all plugin settings from the workspace

Do not copy platform-specific plugin executables as canonical workspace data.

### DaVinci portability

Internal timeline data references artifact IDs/logical URIs.

FCPXML and other path-bearing exports should be regenerated on the current machine so absolute media URLs are always correct for the current Data Root.
