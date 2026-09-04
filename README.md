# OmniCreator

**OmniCreator** is a local-first, plugin-driven YouTube production preparation system.

Its job is not to replace DaVinci Resolve. Its job is to make sure the editor has the **right content, narration, visuals, assets, metadata and timeline structure** before editing begins.

> Prepare cheaply. Compute in bursts. Persist everything. Repeat nothing.

## Product principles

1. **Content relevance first** — visual selection, narration and scene planning optimize for meaning and emotional fit before technical convenience.
2. **Simple by default** — normal users choose a Studio Pack and create. Plugins, routing and compute controls live in Advanced settings.
3. **Local-first control plane** — projects, state, assets and history live on the user's Mac.
4. **Burst GPU compute** — Kaggle T4x2 is treated as a temporary GPU factory, not as the application host.
5. **Plugin-driven capabilities** — voice, visuals, thumbnails, music and export backends can be replaced without changing the core.
6. **DaVinci-first finishing** — OmniCreator prepares assets and timelines; DaVinci Resolve owns editing, effects, grading, mixing and final render.
7. **Resumable by design** — every step/job has persisted state, hashes, attempts and artifacts so work can resume or retry without repeating completed work.
8. **Portable workspace** — all durable creator data lives under a user-selected Data Root and uses logical/relative references, so the workspace can be copied or synchronized to another machine and resumed there.

## Target environment

Current primary setup:

- macOS
- MacBook M2 Pro, 16 GB unified memory
- DaVinci Resolve for final editing
- Kaggle free GPU sessions (T4 x2) for GPU-heavy workloads
- LLMGateway for model/provider routing
- OmniVoiceStudio for narration/TTS
- Pexels as the first stock visual provider
- Future visual providers may include stick-figure animation, illustration, whiteboard, generated video, etc.
- A user-selected portable OmniCreator Data Root may live on local storage or inside a synchronized folder such as Google Drive.

## Architecture at a glance

```text
                        OmniCreator Desktop
                      local Rust/Tauri control
                               |
             +-----------------+------------------+
             |                 |                  |
         LLMGateway        Plugin Runtime      Project State
             |                 |                SQLite
             |          +------+------+             |
             |          |             |             |
             |      VisualProvider  VoiceProvider    |
             |          |             |             |
             |       Pexels        OmniVoice         |
             |          |             |             |
             |          |        Kaggle T4x2         |
             |          |             |             |
             +----------+-------------+-------------+
                                |
                         Local Artifacts
                                |
                        Timeline / SRT / FCPXML
                                |
                         DaVinci Resolve
```

## Documentation

- [Product Vision](docs/00-product-vision.md)
- [System Architecture](docs/01-system-architecture.md)
- [Plugin Architecture](docs/02-plugin-architecture.md)
- [Scene Intelligence & Visual Routing](docs/03-scene-intelligence.md)
- [Kaggle Burst Compute Strategy](docs/04-kaggle-burst-compute.md)
- [State, DAG, Resume & Retry](docs/05-state-resume-retry.md)
- [Domain Model & Project IR](docs/06-domain-model-ir.md)
- [UX, Studio Packs & Advanced Settings](docs/07-ux-studio-packs.md)
- [DaVinci Integration Boundary](docs/08-davinci-integration.md)
- [Plugin API v1 Draft](docs/09-plugin-api-v1.md)
- [Roadmap](docs/10-roadmap.md)
- [Architecture Decisions / Non-goals](docs/11-architecture-decisions.md)
- [Portable Data Root & Device Handoff](docs/12-portable-data-root.md)

## MVP definition

The first useful OmniCreator release should turn a topic/script into a **DaVinci-ready production pack**, not a final rendered video.

Expected output:

```text
project/
├── script/
├── audio/
├── video/
├── images/
├── thumbnail/
├── subtitles/
├── timeline/
└── metadata/
```

The MVP succeeds when a creator can open DaVinci Resolve and start from a largely prepared project instead of a blank timeline.
