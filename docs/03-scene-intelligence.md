# Scene Intelligence and Visual Routing

## Goal

The visual system should select imagery that **communicates the narration**, not simply match keywords.

The critical pipeline is:

```text
Narration
   |
Meaning
   |
Scene Purpose
   |
Emotion
   |
Visual Strategy
   |
Candidate Search/Generation
   |
Content Relevance Ranking
   |
Selection
```

## Scene types

Every scene should be classified into one of the main strategies.

### Literal

The visual can directly show what is being described.

Example:
- person walking through a city
- a storm
- a sunrise

### Emotional

The visual represents the viewer's emotional state.

Example narration:
"Maybe you are tired of carrying everything alone."

Potential visuals:
- person quietly sitting by a window
- empty kitchen late at night
- commuter alone on a platform

### Conceptual

The narration is abstract and benefits from metaphor or animation.

Example:
"Forgiveness is not instant trust."

Potential visuals:
- repairing a bridge
- rebuilding a fence
- slowly opening a previously locked door
- stick-figure boundary illustration

## Anti-cliché rule

Christian long-form content can become visually repetitive if every spiritual sentence maps to:

- praying hands
- open Bible
- church silhouette
- cross
- person staring at sky
- sun rays

These assets are not forbidden, but repeated literal religious imagery should receive a cliché penalty.

Visual freshness and metaphor strength should be scored explicitly.

## Suggested candidate scoring

Default conceptual weighting:

| Dimension | Weight |
|---|---:|
| Semantic relevance | 35% |
| Emotional relevance | 20% |
| Narrative purpose | 15% |
| Visual quality | 10% |
| Channel continuity | 10% |
| Editability/composition | 5% |
| Reuse/freshness | 5% |

Approximately 70% of the score should come from content match.

### Runtime v1 candidate ranking contract

Phase 4 keeps candidate discovery separate from full-asset download.

A provider-normalized `VisualCandidate` contains:

- stable candidate and provider/source identifiers
- an opaque `selection_ref` used only after selection
- generic image/video metadata
- source page and creator attribution metadata
- one or more preview URLs
- no local full-asset path

This makes the default search path preview-first by construction. Full media is resolved only after a candidate is selected.

The default deterministic ranking weights match the table above. Candidate signals are normalized to `0.0..=1.0`; core owns the scoring policy while metadata heuristics or a vision-capable LLM may supply the signals.

Default additional penalties:

- cliché match: `0.08` each, capped at `0.24`
- prior reuse: `0.02` per prior use, capped at `0.10`
- recently used: additional `0.05`

The initial cliché vocabulary follows the documented anti-cliché set and is configurable in the ranking policy. Reuse also lowers the positive freshness component, so frequently reused candidates lose both freshness and an explicit reuse penalty.

Scores expose their full breakdown for review/debugging. Ties are resolved deterministically by stable candidate identity.

## Pexels strategy

Do not immediately download many full videos.

Preferred flow:

```text
SceneIntent
   |
generate multiple search queries
   |
Pexels search
   |
metadata + preview frames
   |
rank candidates
   |
show top candidates / auto-select high-confidence result
   |
download selected full asset only
```

This minimizes bandwidth and local clutter.

### Selected asset fetch contract

Search and download remain separate operations.

`visual.resolve` returns preview-only candidates with an opaque `selection_ref`.
After the user or core selects one candidate, `visual.fetch_selected` receives only that
selection reference plus an optional quality hint.

For Pexels Runtime v1:

- `pexels:video:<id>` is resolved through the Pexels video-by-id endpoint only after selection.
- `pexels:image:<id>` is resolved through the Pexels photo-by-id endpoint only after selection.
- video `quality_mode: "standard"` prefers the smallest downloadable MP4 whose short side is at least 1080 pixels; if no such source exists, it uses the largest available MP4.
- video `quality_mode: "high"` selects the largest downloadable MP4 and is intended for hero shots or heavy crop/reframe workflows.
- selected still images use the original Pexels image source.
- download first lands in the isolated job `temp/` directory and is atomically moved into `output/` only after the transfer completes.
- plugin response returns a relative workspace path plus provider/source/contributor provenance, never a trusted final artifact.
- core validates the returned relative path, verifies the regular file, hashes it, and promotes it through the existing artifact-store boundary.

The current Pexels adapter declares API access plus the direct media hosts used by the documented
photo/video resources. Runtime v1 surfaces those declarations for review; OS-level network
sandboxing remains a later hardening step.

## Search query generation

The LLM should produce multiple concrete visual queries, not spiritual abstractions.

Weak:

```text
God trust patience
```

Better:

```text
woman waiting train station dawn
quiet room rain window night
gardener watering young plant
empty road before sunrise
```

## Vision ranking

If a vision-capable provider is available through LLMGateway, evaluate preview frames before downloading the full asset.

Vision input should include:

- narration
- SceneIntent
- emotional direction
- channel visual rules
- candidates
- avoid/cliché rules

The output should explain why the top result fits.

## Human-in-the-loop

Default user should review meaning, not technical file details.

Example UI:

```text
SCENE 23

Narration:
"You can forgive someone without giving them immediate access..."

Purpose:
Boundary + cautious restoration

A 94%  rebuilding fence
B 87%  closing door
C 82%  walking alone

[Use A]
```

Codec, bitrate and raw API metadata belong in Advanced/Details.

### Runtime v1 review payload

The default review boundary is deliberately human-first:

- show at most three candidates by default, configurable up to six
- keep the deterministic ranking order
- expose one preferred preview, user-facing score, score breakdown and concise rationale
- show cliché/reuse cautions when penalties were applied
- preserve contributor name where available
- expose a recommended candidate as a suggestion only
- always require an explicit selection decision in the default workflow
- omit provider asset IDs, selection references, source/download URLs, dimensions, duration and provider file details

Core retains the complete ranked candidate records. Advanced/Details can resolve a candidate ID back
to its full candidate + score record when technical inspection is actually needed.

This keeps the normal decision focused on meaning and fit rather than media plumbing.

### Optional LLMGateway vision enrichment

Vision review is optional and never replaces the deterministic ranking or the explicit human-selection
boundary.

Runtime v1 sends only the top image previews plus provider-neutral SceneIntent context. It does not
send provider asset IDs, selection references, download URLs or provider-specific metadata to the
vision model.

Before sending a multimodal request, OmniCreator calls LLMGateway route explain with the same request
body. Vision evaluation runs only when the selected route uses API transport. The actual route ID
returned by LLMGateway is checked again after the request; if that route is not confirmed as API
transport, the evaluation is discarded.

This guard is necessary because current browser adapters flatten OpenAI content parts into text and
ignore image_url parts. Treating a browser route as vision-capable would therefore create a false
sense that the model inspected the preview.

When vision enrichment succeeds, it adds a compact fit score and one-sentence rationale to the
existing top-N review card. It does not reorder candidates automatically.

## Runtime v1 stock → generated fallback

Phase 8 P1 adds a provider-neutral routing decision after stock candidate ranking and before any full-asset fetch.

For normal scene visuals:

1. stock discovery/ranking remains preview-first,
2. core compares the best ranked stock candidate with a configured quality threshold,
3. a viable stock candidate stays on the existing human review/selection path,
4. no candidates, unavailable stock discovery, or a below-threshold best candidate routes to `generated_still`,
5. the decision records only stable core fields such as scene ID, score, threshold, route and reason; provider IDs, selection references, API details and credentials are not part of the routing contract.

The routing decision is persisted in canonical artifact metadata by core. Stock plugins still return workspace outputs that are verified/promoted through ArtifactStore; generated plugins still use the canonical Job/Attempt/Artifact path. There is no fallback-specific database or scheduler.

Thumbnail backgrounds use the same `visual.generate` operation, generated-image request contract, job/attempt state and ArtifactStore boundary. Core marks the execution use case as `thumbnail_background` and persists that provenance; it does not introduce a separate thumbnail generation subsystem.

## Mixed visual routing

A video does not need one provider for every scene.

Example hybrid pack:

```text
SC01 emotional   -> Pexels
SC02 conceptual  -> Stick Figure
SC03 scripture   -> Typography
SC04 historical  -> Illustration
SC05 emotional   -> Pexels
```

This is a major design goal.

## Asset resolution strategy

Technical selection happens only after content selection.

Default media resolution should be adaptive:

- standard B-roll: 1080p preferred
- heavy crop/reframe: higher resolution preferred
- hero visual: best suitable source
- still image: high-resolution original
- 4K should not be downloaded automatically when it adds no editing value

## Local library

The first version only needs metadata indexing:

- source
- path
- tags
- emotional tags
- scene type
- usage count
- last used
- quality
- perceptual/file hash if useful

Semantic embeddings can be added later.

The resolver should search the curated local library before external providers when appropriate, while applying recency/reuse penalties.

## Channel Visual Bible

Studio/Channel profiles should define reusable visual rules:

- audience age/style
- lighting
- mood
- camera feel
- preferred environments
- visual density
- typography
- avoid list
- cliché rules

Scene resolution inherits these rules automatically.

## Phase 11 stick figure semantic projection

The first stick-figure provider consumes the existing `SceneIntentV1` embedded in the existing `visual.generate` request. It does not introduce a new scene schema.

For each scene it deterministically derives a renderer-local semantic plan:

- `characters`: small archetypal roles such as person, friend, parent, child, guide or builder
- `actions`: explanatory actions such as explain, repair, rebuild trust, set boundary, support, walk, wait or pray
- `objects`: simple visual metaphors such as bridge, fence, door, path, book, heart, boundary, box or light

The plan is plugin metadata only. Core persists the resulting artifact/provenance through the existing ArtifactStore boundary and does not adopt these renderer-local fields as canonical workflow state.

The checked-in reference renderer is procedural SVG, deterministic for identical request + seed, offline and restricted to the granted job workspace. `christian-stick-explainer` / `stick-figure-minimal-motion` use a minimal animated SVG treatment. `stick-figure-thumbnail` is a thumbnail-specific composition preset. A richer whiteboard renderer remains deferred.
