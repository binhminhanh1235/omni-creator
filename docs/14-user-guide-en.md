# OmniCreator User Guide (English)

This guide describes how to use the current OmniCreator desktop application and the verified Phase 0-16 workflow. It is written for creators first, while also documenting the advanced controls needed to recover work when providers, plugins, GPU compute, LLMs, or external services are unavailable.

> Core operating rule: automatic execution is an accelerator, not a single point of failure. Every production stage has a recovery or manual continuation path, and accepted results converge on the same canonical Project, WorkflowStep, Job, Attempt, Artifact, and Production Pack state.

## 1. What OmniCreator does

OmniCreator is a local-first, plugin-driven production preparation system for YouTube and similar video workflows. It prepares the material that should exist before creative editing begins:

- creator topic or script
- canonical content and segments
- scene plan / SceneIntent
- stock, generated, stick-figure, library, or manually supplied visuals
- narration audio and timing
- artifact provenance and source information
- resumable workflow state
- subtitles and timeline/interchange data
- a DaVinci-ready Production Pack

OmniCreator does **not** replace DaVinci Resolve. Final editing, effects, grading, mixing, and final rendering remain in the editor.

## 2. Current delivery state and launching the app

The repository currently has no published GitHub Release. The checked-in desktop is a Tauri 2 application with a static frontend under `apps/desktop/dist`.

For development/source use you need:

- macOS is the primary target environment
- Rust 1.80 or newer
- a working Rust/Cargo toolchain
- platform requirements required by Tauri 2
- Python 3 if you intend to execute the checked-in Python visual plugins directly

From the repository root, the desktop can be launched from the Tauri crate during development:

```bash
cargo run --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Core verification commands used by CI are:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
```

The normal creator workflow below assumes the app is already running.

## 3. Concepts worth understanding first

### Data Root

The Data Root is the durable OmniCreator workspace. Projects, SQLite state, artifacts, exports, library media, Studio Packs, and portable metadata live under this folder. Canonical project data uses logical references rather than depending on one machine's absolute path.

### Studio Pack

A Studio Pack is the creator-facing production style and routing policy. It defines capability-oriented routes, automation level, presets, and quality thresholds. You normally choose a Studio Pack instead of wiring providers manually.

### Project Kanban

The project board is derived from canonical workflow state. Current columns are:

- Ideas
- Preparing
- Needs Review
- GPU Ready
- GPU Running
- Ready to Edit
- Done

Board placement is not separate saved UI state. It is reconstructed from project/workflow/job state after restart or workspace handoff.

### Review Center

Review Center is the main exception and recovery surface. It tells you what is blocked, failed, unavailable, missing, or invalid, and exposes only actions that are valid for the current condition.

### Production Pack

The Production Pack is the verified bundle and metadata used to enter DaVinci Resolve close to the creative-editing stage. It is assembled only from selected and physically verified canonical inputs.

## 4. First launch and Data Root setup

On first launch OmniCreator asks:

- **Create New Data Folder**
- **Use Existing Data Folder**

Choose **Create New Data Folder** for a fresh workspace. Choose **Use Existing Data Folder** when opening a previously created OmniCreator Data Root on this machine or another machine.

A healthy workspace contains an `.omnicreator/workspace.json` manifest and durable state under the Data Root. Typical durable content includes project state, media, artifacts, Studio Packs, library assets, exports, and clean handoff snapshots.

### If another writer may still be active

OmniCreator intentionally avoids automatically opening two writers against a synchronized workspace. If a possible conflict is detected, use one of the offered actions:

- **Open Read Only** to inspect safely
- **Check Again** after the other machine has closed and synchronization finishes
- **Choose Different Folder** if you selected the wrong workspace

Read-only mode lets you inspect projects, Review Center, Production Pack/recovery health, and details, but mutation actions are disabled and backend writable guards remain authoritative.

## 5. Main workspace and project board

Each project card exposes a concise status, Studio Pack, workflow stage strip, and the next primary action. The main semantic stages are:

1. Content
2. Scenes
3. Visuals
4. Voice
5. Production Pack

Depending on state, the primary project action can appear as:

- **Start / Resume**
- **Review**
- **Run GPU**
- **Sync / Resume**
- **Assemble**
- **Review / Export**

Project cards also expose **Studio Pack**, **Production Status** or **Export to Resolve**, plus Rename/Delete when the workspace is writable.

## 6. Configure integrations and plugins

You do not need every integration to complete a project. Configure only the automatic capabilities you want to use. Missing capabilities should become setup/recovery actions rather than permanent project dead ends.

### LLMGateway

LLMGateway is used for automatic creator content tasks and SceneIntent generation. Use the **LLMGateway** panel to configure/check the local gateway connection.

If LLMGateway is unavailable, you can still continue by providing the script and scene plan manually. Provider/model/session details must not be stored as portable Project state.

### Plugin Manager

Open **Plugin Manager** from the workspace settings area. The current UI groups plugins into:

- **Installed / Enabled**
- **Disabled**
- **Needs Attention**

Built-in plugins can be enabled/disabled but are application-managed. User-installed plugins may support inspected update and uninstall flows. Plugin installation/lifecycle state is machine-local, not portable project truth.

### Current checked-in visual providers

The repository includes the following visual plugin implementations:

| Provider | Main use | Default machine-local secret/config reference |
| --- | --- | --- |
| Pexels | Stock image/video, preview-first | `PEXELS_API_KEY` |
| Pixabay | Stock image/video, preview-first | `PIXABAY_API_KEY` |
| Unsplash | Stock photos with attribution and selected-use tracking | `UNSPLASH_ACCESS_KEY` |
| Storyblocks | Commercial stock image/video | `STORYBLOCKS_PUBLIC_KEY`, `STORYBLOCKS_PRIVATE_KEY`, `STORYBLOCKS_USER_ID`, `STORYBLOCKS_API_MODE` |
| Generated Image API | API-backed generated stills | `OPENAI_API_KEY` by default; endpoint/model configurable inside plugin settings |
| Generated Image Reference | Offline/reference generated-still capability | No provider secret required by the reference implementation |
| Stick Figure Reference | Offline stick-figure visual capability | No provider secret required by the reference implementation |

Secrets stay machine-local. Do not paste secrets into Project, Studio Pack, ScenePlan, external-handoff JSON, or the portable Data Root as canonical state.

For Storyblocks, test credentials may search/preview, but promotable selected production assets require production API mode and compatible licensing.

### Voice and compute

Automatic voice production uses the configured voice runtime and, when required, ComputeProvider/GPU readiness. If that path is not available, import manual audio and timing instead. A project does not need a functioning GPU provider to be completable.

## 7. Choose a Studio Pack and create a production

The creator-facing settings model has three layers.

### Basic

Use Basic for normal creation. Choose the creator input and an available Studio Pack, then create/start the production. A Studio Pack can be:

- `AVAILABLE`
- `AVAILABLE_WITH_SETUP`
- `UNAVAILABLE`

Availability is derived from actual installed capabilities and machine-local readiness. A pack definition alone does not make a missing capability real.

### Customize

Use Customize for high-value controls such as:

- curated presets
- automation level
- quality thresholds

Project customization remains portable and extends the selected built-in Studio Pack rather than inventing another routing system.

### Advanced

Use Advanced to inspect resolved plugin/capability routes, target order, value sources, diagnostics, and existing machine-local runtime controls. Provider endpoints, credential values, and absolute paths are not stored in portable Studio Pack state.

### Automation levels

- **Assisted**: do not auto-advance creator review checkpoints.
- **Balanced**: automate deterministic/low-risk work and pause for meaningful or ambiguous decisions.
- **Autopilot**: advance low-risk work automatically and stop on blockers/high-impact exceptions.

All automation levels still respect capability/setup failures and the canonical workflow state machine.

## 8. Automatic creator flow

A normal automatic flow is:

```text
Topic or Script
  -> Content
  -> Scene Plan / SceneIntent
  -> Visual preparation and selection
  -> Voice / timing
  -> Production Pack
  -> Resolve export
```

### Content

In topic mode, OmniCreator may use LLMGateway to produce canonical creator content. In script mode, creator text can be preserved directly rather than unnecessarily rewritten.

### Scene planning

Canonical segments are created before SceneIntent. Scene planning keeps segment identity and narration linkage stable so downstream visuals and voice can be replaced precisely.

### Visuals

Visual routing follows the resolved Studio Pack order. Stock routes use preview-first discovery and selection before full media download. Generated/stick routes respect approval rules based on automation level.

Automatic and manual visuals can be mixed scene-by-scene.

### Voice

Voice work is tracked per segment. Successful verified segment audio/timing can be reused; changing one segment does not require recreating every other segment.

### Production Pack

Production Pack assembly reads only verified selected content, SceneIntent, visuals, narration audio, and timing. Voice timing is the stable clock used for narration/visual duration and subtitle offsets.

## 9. Universal manual takeover

Manual input is not a side database or an emergency hack. A valid manual result is validated, recorded as canonical Job/Attempt history with provenance, promoted through ArtifactStore, verified, and then unlocks downstream DAG work.

### 9.1 Manual Script

When content cannot be generated automatically, use:

- **Provide Script Manually**
- **Import TXT/Markdown**

The supplied script becomes canonical Content through the normal ingestion boundary. It does not fake an LLM provider success.

Use this when:

- LLMGateway is offline or not configured
- you already wrote the script elsewhere
- you want exact creator-authored wording

### 9.2 Manual Scene Plan

After Content succeeds, use **Edit Scene Plan Manually**. The editor supports:

- **Use Manual Scene Plan**
- **Import ScenePlan JSON**

The import/editor derives or validates canonical identities, schema, linkage, narration, and hashes. A manual Script plus manual ScenePlan can unlock Visuals without LLMGateway.

### 9.3 Manual Visual per scene

For any scene you can use manual visual takeover instead of waiting for a stock/generated provider. Current flows include:

- choose an existing Asset Library item
- use your own image
- use your own video
- **Provide Manually**
- **Replace Existing Visual**

Imported media is copied into the controlled workspace, hashed, minimally inspected, verified, promoted to ArtifactStore, and bound to the correct scene. Replacing one scene invalidates only the appropriate downstream dependency cone.

### 9.4 Manual Voice and timing per segment

When automatic voice/TTS/GPU is unavailable, provide your own narration per segment. The current recovery path supports manual audio and timing, including replacing an existing voice result.

Use supported audio such as WAV/MP3 and provide compatible timing/SRT information. Timing is validated for non-negative ordered timing, duration, and segment linkage.

This enables:

```text
manual audio for Segment A
+ automatic voice for Segment B
+ corrected timing for Segment C
```

without rebuilding unrelated verified voice work.

### 9.5 External generated/compute handoff

When a generated-image or voice/compute request is prepared but the local provider/runtime cannot execute it, OmniCreator can export a provider-neutral request for execution elsewhere.

Typical actions are conceptually:

- Copy Request
- Export Request
- inspect Details
- Provide External Result
- Replace External Result

Important rules:

- exported requests do not contain secrets, provider-private fields, or absolute machine paths
- request hashes are checked again before import
- stale results cannot attach to newer SceneIntent/Content
- imported external results record external/manual provenance
- a failed plugin/ComputeProvider attempt is not rewritten as successful

## 10. Review Center and recovery actions

Review Center projects canonical blockers and recovery health. Its action vocabulary is:

- **Retry**: prepare/requeue a real retryable canonical Job
- **Configure**: go to the owning setup surface, such as LLMGateway/plugin/runtime configuration
- **Choose Alternative**: only when at least two ready candidates satisfy the exact required capability and product semantics require an explicit choice
- **Provide Manually**: open the applicable manual takeover path
- **Replace Result**: repair/relink missing or invalid selected artifacts
- **Details**: inspect stage/item identity, blocker classification, reason, source, health, and applicable actions

Review Center recommends a primary action instead of flooding each card with every possible control.

A deterministic Studio Pack fallback is not presented as a fake provider-choice menu. Exact capability semantics matter. For example, generic generated still capability is not automatically treated as equivalent to a stick-figure semantic route.

## 11. GPU Workbench and burst compute

GPU Workbench is designed to make expensive compute a batch operation rather than a permanent dependency.

Recommended workflow:

1. select one or more projects using the **GPU BATCH** checkboxes
2. prepare/review all possible non-GPU dependencies locally
3. inspect the prepared GPU queue and blockers
4. connect the configured ComputeProvider only when the queue is worthwhile
5. run the GPU work
6. synchronize verified outputs back into canonical local state
7. retry only remaining/retryable work if the remote worker disappears

The advanced provider details are machine-local. Store environment-variable names/references rather than secret values in portable configuration.

If GPU compute is unavailable, use manual audio/timing or external handoff where applicable. Do not treat GPU availability as a prerequisite for finishing every project.

## 12. Asset Library

The Asset Library provides reusable canonical media with metadata, tags, source reuse information, and usage history.

Use it to:

- reuse already verified assets
- avoid downloading or regenerating identical media
- select a library asset for a scene during manual takeover
- inspect provenance/source identity
- identify exact duplicates or reused provider assets

A library selection still binds the chosen artifact to the target scene through canonical state.

## 13. Production Pack and DaVinci Resolve export

When all required inputs are verified, open **Production Status** / **Export to Resolve**.

The Production Pack panel is a controller over canonical export state. It can expose:

- current Production Pack state
- portable `ProductionPackV1` JSON for inspection
- Export / Regenerate controls
- logical package location
- cache-hit feedback
- Job/Attempt history
- missing-artifact/relink diagnostics

Normal usage does not require hand-editing Production Pack JSON.

### Export/rebuild sequence

```text
verified content + scenes + visuals + audio + timing
  -> Production Pack
  -> subtitles / timeline / source report / interchange
  -> Resolve export
```

On another machine, path-bearing interchange should be regenerated from the current Data Root binding rather than assuming old absolute file URLs remain valid.

## 14. Production recovery and relinking

If selected media is missing or hash-invalid, Production Recovery derives the problem from canonical artifact state. Supported repair actions include visual, audio, timing, and combined audio+timing repair/relink, followed by rebuilding the Production Pack and regenerating Resolve output.

Recovery works for automatic, manual, and external artifacts through the same canonical path.

Replacing a broken artifact should invalidate the affected dependency cone only. Unrelated verified work should remain reusable.

## 15. Restart, resume, retry, and cache behavior

OmniCreator is designed to resume from persisted state rather than rerun completed work.

After restart:

- project board state is reconstructed
- Job/Attempt history remains visible
- interrupted work is reconciled according to canonical state
- verified artifacts can be reused
- failed/retryable work remains actionable
- Production Pack history can be reloaded

Do not delete the Data Root or internal SQLite files to fix a normal workflow blocker. Use Review Center, retry, replacement, recovery, or a clean workspace handoff instead.

## 16. Moving the Data Root or switching machines

The safe handoff flow is:

1. finish or pause work on Machine A
2. use **Prepare for Device Handoff** or close OmniCreator cleanly
3. allow the Data Root to finish copying/synchronizing
4. on Machine B, launch OmniCreator and choose **Use Existing Data Folder**
5. wait for workspace/state/artifact validation
6. install any missing machine-local plugins if needed
7. configure missing credentials if you want those automatic providers
8. continue existing projects

For cloud-synchronized folders, do not use simultaneous writers. File synchronization is not a distributed database.

If active media is online-only, make it available offline before editing in DaVinci when possible.

### What moves with the Data Root

Portable state includes project definitions, workflow/job/attempt history, media/artifacts, voice takes, captions/timing, Studio Packs, provenance, export source data, and clean state backups.

### What should be reconfigured per machine

Machine-local items include plaintext secrets, OS Keychain content, disposable caches, runtime binaries/environments, DaVinci application path, and per-device GPU preferences.

## 17. Fully manual workflow

OmniCreator is verified to support a complete project without LLM, stock provider, generated-image provider, voice provider, GPU, or other external service.

A practical fully manual recipe is:

```text
Create project with an applicable Studio Pack/workflow
  -> Provide Script Manually or Import TXT/Markdown
  -> Edit / Import Manual Scene Plan
  -> Provide image/video for every scene
  -> Provide narration audio for every segment
  -> Provide/correct timing
  -> Assemble/Rebuild Production Pack
  -> Export to Resolve
```

All outputs still use canonical validation, provenance, ArtifactStore promotion, Job/Attempt history, dependency invalidation, restart/resume, and portable logical references.

## 18. Mixed workflow examples

### Example A: LLM content, manual visuals

Use LLMGateway for Content and Scenes, then use your own images/video for selected scenes. Continue automatic voice, then export.

### Example B: Manual script, automatic stock

Import your finished script, create/import ScenePlan manually, then let Pexels/Pixabay/Unsplash/Storyblocks routes provide stock candidates.

### Example C: Automatic visuals, manual voice

Complete Content, Scenes, and Visuals automatically, then import your own narration and timing per segment.

### Example D: External image generation

Prepare generated-visual requests, execute them with another allowed external tool/service, import the matching results, then continue Voice and Production Pack.

## 19. Troubleshooting

| Symptom | Recommended action |
| --- | --- |
| LLMGateway unavailable | Configure LLMGateway, retry, or provide Script + ScenePlan manually |
| Studio Pack unavailable | Open Studio Pack diagnostics / Plugin Manager and inspect the exact missing capability |
| Stock API key missing | Configure the provider's machine-local environment variable, or provide visuals manually |
| Storyblocks can preview but not promote | Verify licensed production credentials and `STORYBLOCKS_API_MODE=production` |
| Generated provider unavailable | Use a manual image, Asset Library, another valid route, or external handoff |
| Voice/GPU unavailable | Import manual audio + timing per segment or use external handoff if prepared |
| One visual/audio file is missing or invalid | Use Review Center `Replace Result` / Production Recovery relink for that item |
| Project stuck after failure | Open Review Center, inspect Details, then Retry/Configure/Provide Manually as applicable |
| Workspace says another writer may be active | Open Read Only or close the other machine and Check Again after sync |
| Moved Data Root has old Resolve paths | Regenerate the Production Pack / Resolve export from the current binding |
| Cloud file exists but is not local | Hydrate/download the file first; avoid unnecessary regeneration |

## 20. Safety and data integrity rules

For reliable projects:

- keep one writer per Data Root
- do not edit canonical SQLite state manually
- do not mark workflow steps successful by hand
- do not store API keys/tokens/cookies inside portable project data
- do not import stale external results against changed content/SceneIntent
- use app-level replace/relink actions instead of replacing files behind ArtifactStore
- keep manual/external inputs inside the supported validation path
- close cleanly before switching machines
- let cloud synchronization complete before opening the workspace elsewhere

## 21. Provider credential quick reference

```text
Pexels
  PEXELS_API_KEY

Pixabay
  PIXABAY_API_KEY

Unsplash
  UNSPLASH_ACCESS_KEY

Storyblocks
  STORYBLOCKS_PUBLIC_KEY
  STORYBLOCKS_PRIVATE_KEY
  STORYBLOCKS_USER_ID
  STORYBLOCKS_API_MODE=production   # required for promotable production downloads

Generated Image API (default settings)
  OPENAI_API_KEY
```

Environment variable names can be configured in the relevant plugin's advanced settings where the schema allows it. The value itself remains machine-local.

## 22. Where to read deeper technical documentation

For implementation and architecture details, see:

- `docs/00-product-vision.md`
- `docs/01-system-architecture.md`
- `docs/02-plugin-architecture.md`
- `docs/03-scene-intelligence.md`
- `docs/04-kaggle-burst-compute.md`
- `docs/05-state-resume-retry.md`
- `docs/06-domain-model-ir.md`
- `docs/07-ux-studio-packs.md`
- `docs/08-davinci-integration.md`
- `docs/09-plugin-api-v1.md`
- `docs/10-roadmap.md`
- `docs/11-architecture-decisions.md`
- `docs/12-portable-data-root.md`
- `docs/13-review-center-recovery.md`

For most creators, this User Guide plus Review Center should be enough for daily operation. Use the architecture documents when diagnosing integrations, plugin development, portability rules, or workflow contracts.
