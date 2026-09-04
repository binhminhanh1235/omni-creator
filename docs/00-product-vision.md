# Product Vision

## What OmniCreator is

OmniCreator is a **local-first YouTube production preparation system**.

It does not try to replace DaVinci Resolve. Its responsibility ends when the creator has the best possible production ingredients:

- research and script
- narration
- scene intent
- relevant stock/generated visuals
- thumbnail assets
- captions
- source/license metadata
- timeline structure
- a DaVinci-ready project pack

The product should feel simple enough that a normal user can create a project from a topic and a Studio Pack, while still giving advanced users deep control over providers, routing, plugins, models and compute.

## Primary goal

**Optimize for content relevance first.**

Technical convenience, codec choice, local compute usage and GPU utilization are secondary. A visually beautiful but semantically weak clip is a bad result.

Visual selection should prioritize:

1. semantic relevance
2. emotional relevance
3. narrative purpose
4. freshness / cliché avoidance
5. continuity with the channel
6. visual quality
7. editability / composition
8. file technical suitability

## Current target workflow

Primary environment:

- MacBook M2 Pro, 16 GB unified memory
- local OmniCreator desktop app
- DaVinci Resolve for final editing/render
- Kaggle T4 x2 for burst GPU work
- LLMGateway for model/provider routing
- OmniVoiceStudio for TTS
- Pexels for initial stock footage
- generated image providers as fallback
- future plugin styles such as stick-figure animation

## Core product promise

A creator should be able to prepare many projects before starting a Kaggle session.

Example:

```text
Project 01  GPU READY
Project 02  GPU READY
Project 03  GPU READY
...
Project 10  GPU READY

START KAGGLE
    |
    +--> TTS batch
    +--> generated-image batch
    +--> optional heavy AI batch

results sync immediately to local projects

READY FOR DAVINCI
```

This turns limited free GPU time into mostly useful compute instead of setup, waiting and manual interaction.

## Product principles

### 1. Simple outside, powerful inside

Default flow:

```text
Topic / Script
Studio Pack
Duration
Voice

[ Create Production ]
```

Advanced settings expose:

- plugin/provider selection
- routing
- model selection
- fallback chains
- Kaggle compute details
- cache policies
- concurrency
- export settings

### 2. Human reviews meaning, not infrastructure

The user should review:

- script
- visual concept
- scene relevance
- thumbnail direction
- voice quality

The user should not normally review:

- bitrate
- codec
- job IDs
- worker internals
- hashes
- dependency graph

### 3. DaVinci is the finishing system

OmniCreator prepares. DaVinci edits.

OmniCreator does not own:

- final render
- grading
- motion effects
- audio mixing
- proxy generation by default
- general video encoding

### 4. Plugins replace capabilities, not the core

The same SceneIntent can be rendered by:

- Pexels stock footage
- local stock library
- generated image
- stick-figure animation
- whiteboard animation
- future video generator

The core must not know which implementation is used.

### 5. Repeat nothing

Every expensive deterministic/reproducible operation should use input hashes and artifact tracking.

Changing one segment must not regenerate an entire project.

### 6. Persist everything important

Project state lives locally and survives:

- OmniCreator restart
- Kaggle disconnect
- quota exhaustion
- network failure
- individual model failure

## Long-term direction

OmniCreator should become a reusable production operating system for multiple channel styles, not only the initial Christian long-form workflow.

Potential Studio Packs:

- Christian Cinematic
- Christian Stick Explainer
- Bible Illustrated
- Night Devotional
- Sleep Scripture
- Psychology Explainer
- Technical Explainer
- Educational Whiteboard

The project should evolve by adding/replacing plugins and Studio Packs rather than forking the core.
