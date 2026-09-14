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
9. **One canonical control plane** — Desktop, CLI and MCP/agent harnesses operate the same Project / WorkflowStep / Job / Attempt / ArtifactStore state instead of maintaining transport-specific workflow truth.
10. **Manual takeover is universal** — unavailable LLM/provider/plugin/TTS/GPU execution must remain recoverable through canonical manual/external result paths.

## Target environment

Current primary setup:

- macOS
- MacBook M2 Pro, 16 GB unified memory
- DaVinci Resolve for final editing
- Kaggle free GPU sessions (T4 x2) for GPU-heavy workloads
- provider-neutral LLM execution through LLMGateway, direct OpenRouter, or another configured OpenAI-compatible endpoint
- OmniVoiceStudio for narration/TTS
- Pexels as the first stock visual provider
- Future visual providers may include stick-figure animation, illustration, whiteboard, generated video, etc.
- A user-selected portable OmniCreator Data Root may live on local storage or inside a synchronized folder such as Google Drive.
- Optional CLI/MCP control from Codex, Claude Code, Google Antigravity, shell scripts, and other agent harnesses through the same Rust `omnicreator` executable.

## Architecture at a glance

```text
                 Desktop / CLI / MCP agents
                           |
               Application Control Service
                           |
                  OmniCreator Core
            Project / Workflow / Jobs / State
             /             |               \
   LLM Provider       Plugin Runtime    ComputeProvider
   /       \          /          \          |
LLMGateway OpenRouter Visual      Voice   Kaggle T4x2
                           |
                    Local Artifacts
                           |
                   Production Pack
                           |
                    DaVinci Resolve
```

Desktop, CLI and MCP do not own separate schedulers or workflow databases. LLM/provider selection is machine-local execution configuration; portable creator state stays provider-neutral.

## Agent control

OmniCreator exposes deterministic CLI and local stdio MCP modes from the same Rust binary. Checked-in harness packages provide safe setup for Codex, Claude Code and Google Antigravity:

- [Agent harness setup](agent-harness/README.md)
- [Agent harness setup — Tiếng Việt](agent-harness/README.vi.md)
- [CLI examples for agents](agent-harness/CLI-EXAMPLES.md)
- repository skill for Codex/Antigravity: `.agents/skills/omnicreator/SKILL.md`
- repository skill for Claude Code: `.claude/skills/omnicreator/SKILL.md`

The agent safety contract is inspect first, mutate through typed canonical controls only, re-inspect after every mutation, and never fabricate success or edit SQLite/ArtifactStore internals directly.

## Documentation

### User guides

- [Complete User Guide (English)](docs/14-user-guide-en.md)
- [Hướng dẫn sử dụng đầy đủ (Tiếng Việt)](docs/15-user-guide-vi.md)

### Architecture and implementation

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
- [Review Center Universal Recovery](docs/13-review-center-recovery.md)
- [Application Control Service](docs/16-application-control-service.md)
- [Phase 18 control-plane roadmap](docs/17-phase18-roadmap.md)
- [CLI control surface](docs/18-cli-control.md)
- [MCP stdio control surface](docs/19-mcp-control.md)
- [Provider-neutral LLM runtime](docs/20-llm-providers.md)
- [Agent harness packages](docs/21-agent-harnesses.md)

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
