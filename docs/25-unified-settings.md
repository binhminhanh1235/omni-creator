# Unified Settings and bundled built-in plugins

Phase 20 consolidates machine-local configuration into a single Desktop **Settings** surface.

## Settings sections

### Workspace

The Workspace section shows the current Data Folder, workspace identity, revision, writer/read-only state, device-handoff control and the existing Change Data Folder action.

Canonical project data still belongs to the portable Data Root. Opening Settings does not create a second settings database.

### AI & Runtime

This section contains the existing LLM Provider configuration and OmniVoiceStudio runtime configuration.

The LLM Provider keeps its current endpoint, model and API-key-environment-variable behavior. OmniVoiceStudio keeps its runtime URL normalization, health/capability checks and bearer-token-environment-variable behavior.

Credential values remain machine-local. OmniCreator stores environment-variable names and machine-local runtime settings only; secret values are not written to the Data Root or canonical project artifacts.

Review Center `Configure LLM` actions open this Settings section directly.

### Plugins

The Plugins section contains the existing Plugin Manager inventory, readiness, capability and lifecycle controls. Local user-installed plugins still use the existing guarded install/update/uninstall flows and capability-impact previews.

## Built-in plugins

The Desktop bundle now packages the checked-in `plugins/` directory into the application resource root as `plugins/`. The existing runtime already scans `resource_dir()/plugins`, so packaged built-ins use the same canonical `PluginRegistry` as development-time built-ins.

Built-in plugins are enabled by default because `PluginLifecycleStateV1` starts with an empty disabled set. A user may still disable a built-in plugin machine-locally. Built-ins remain app-managed and cannot be uninstalled.

The currently bundled checked-in providers include:

- Generated Image API
- Generated Image Reference
- Pexels
- Pixabay
- Stick Figure Reference
- Storyblocks
- Unsplash

Bundling a plugin does **not** bundle its provider credential. A plugin that requires an API key or licensed account continues to report `SETUP REQUIRED` until the required environment variables are present.

## Portability boundary

The following remain machine-local and are not copied by moving the Data Root:

- LLM provider endpoint/model/credential environment binding
- OmniVoiceStudio runtime/tunnel URL and bearer-token environment binding
- plugin enabled/disabled lifecycle state
- provider credential values
- user-installed plugin binaries/directories

Portable Project, WorkflowStep, Job, Attempt, ArtifactStore and Studio Pack state is unchanged.
