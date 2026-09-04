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
- source page metadata
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
