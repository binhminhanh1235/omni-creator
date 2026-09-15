# Parallel visual + voice production recipe

Tracking: Phase 19 P4 #118.

This recipe turns the Phase 19 worker-pool contract into a practical visual/voice split without creating a second workflow engine. OmniCreator remains the authority for readiness, selected canonical artifacts, stale detection, recovery, and final fan-in.

## Dependency gates

Voice segments become externally executable after verified Content. Visual scenes become externally executable after verified ScenePlan. Therefore a coordinator can start voice work while ScenePlan is still being produced, then add visual workers as soon as ScenePlan commits.

```text
Content --------------------------+--> voice segment workers
   |                              |
   `--> ScenePlan --> visual scene workers
                    \             /
                     canonical fan-in
                           |
                     ProductionPack
```

Always rebuild the worker queue from `agent_work_graph`; do not infer readiness from a harness transcript.

## Visual dispatch: do not collapse three different lanes

### 1. Stock lane: Pexels example

Pexels is a stock provider, not a `visual.generate` provider. Its manifest advertises `stock_video`, `stock_image`, `preview_first_search`, and `selected_asset_download`.

Use this sequence when canonical Studio Pack intent and available runtime capability choose stock:

1. Search with `visual.resolve` using scene search intent.
2. Keep discovery preview-first. Inspect normalized candidate metadata, previews, creator/source metadata and `selection_ref` values.
3. Select one candidate. Do not download every full-resolution search result.
4. Fetch only the selected candidate with `visual.fetch_selected`.
5. Commit the chosen image/video through a canonical OmniCreator visual path.

When the machine-local OmniCreator plugin runtime owns stock execution, keep selection/fetch inside that runtime and its canonical visual orchestration.

When a harness performs Pexels as an external fallback, ingest the selected local image/video with `visual_control` action `provide_manual` using `ManualResultProvenanceV1` producer `EXTERNAL_RESULT_IMPORT` and a truthful symbolic source label such as `pexels`. This path supports both image and video media.

Do **not** use `agent_work_commit` with an `ExternalGeneratedVisualRequestV1` to submit a Pexels video. The Phase 19 external visual descriptor is currently the generated-still contract and accepts PNG/JPEG/WebP stills only.

If Pexels returns a rate-limit response, preserve it as retryable, honor its `retry_after_seconds`, and either retry later or explicitly choose another allowed route. Rate limiting is not success.

### 2. Generated-still lane

When the prepared visual work item contains the `visual.generate` external descriptor, a worker may render the requested still image. The result must satisfy the generated external result contract: PNG, JPEG, or WebP.

The coordinator commits it through `agent_work_commit` or the equivalent `visual_control` external handoff. The prepared `request_sha256` guards against ScenePlan changes while the worker was running.

Generated-still execution does not imply stock capability, and it must not be used to smuggle video into the still-image contract.

### 3. Manual visual lane

When stock/generated capability is unavailable, or a human has chosen a better asset, ingest the image/video through the canonical manual visual operation. Keep truthful provenance and use explicit replacement if a verified result already exists.

Manual takeover is a supported production route, not a hidden workaround.

## Voice dispatch: OmniVoiceStudio

For each READY voice work item:

1. The coordinator calls `agent_work_prepare` for the canonical voice `work_id`.
2. Pass the returned `tts.generate` descriptor to an OmniVoiceStudio worker. The descriptor contains canonical `project_id`, `segment_id`, narration, voice direction, result requirements, and `request_sha256`.
3. OmniVoiceStudio renders WAV or MP3 plus timing for the same segment. Timing is required and must be SRT or OmniCreator VoiceTiming JSON compatible with the canonical segment.
4. The worker returns candidate paths/result metadata to the coordinator. It does not mutate OmniCreator.
5. The coordinator submits the result through `agent_work_commit` with source label `omnivoice-studio`.
6. Re-run `agent_work_graph` immediately after commit.

If Content changes while rendering, OmniCreator must reject the obsolete request hash as stale. Discard the old output as a current result and prepare the new work identity.

If OmniVoiceStudio or its compute route is unavailable, use `voice_control` action `provide_manual` with a valid audio + timing bundle. Do not mark the voice unit complete merely because an external renderer finished a file.

## Parallel execution policy

Provider concurrency is operational configuration, never portable Project truth.

- Keep total worker count bounded by the P3 worker-pool policy.
- Pexels search/fetch concurrency should remain conservative and must honor provider retry guidance such as `retry_after_seconds`.
- OmniVoiceStudio concurrency should reflect the actual local/remote runtime capacity, including GPU/ComputeProvider limits.
- Credentials, provider paths, device IDs and rate-limit state stay machine-local.
- Canonical commits are still serialized through the coordinator and Data Root writer lease.

A useful scheduling shape is to keep voice workers running continuously once Content exists, while scene visual workers arrive in waves after ScenePlan. Do not wait for every voice segment before starting visuals, and do not wait for every visual before rendering voice.

## Mixed fallback examples

**Pexels unavailable:** keep the provider failure truthful. If Studio Pack/canonical review permits a generated still for that scene, dispatch generated visual work; otherwise supply a manual visual.

**Pexels search succeeds but no candidate is good enough:** do not fetch arbitrary full media just to create success. Choose another provider/route or manual takeover.

**Generated provider unavailable:** provide a supported external/manual still image instead of converting the failed provider Attempt to success.

**OmniVoiceStudio unavailable:** provide a manually rendered audio + timing bundle and keep provenance truthful.

**One worker fails while others succeed:** commit only accepted completed candidates, re-inspect, and redispatch the still-current missing work. P5 adds deeper crash/contention/fan-in hardening; P4 does not invent a second worker database.

## Fan-in gate

Before ProductionPack:

1. re-inspect `agent_work_graph`;
2. inspect `visual_control status` and `voice_control status` when diagnosing per-unit gaps;
3. inspect Review Center/recovery for failed/stale units;
4. require verified canonical visual + voice aggregate completion;
5. only then assemble/rebuild/export ProductionPack.

Worker completion, a Pexels download, or an OmniVoiceStudio audio file is evidence of external execution only. Canonical acceptance by OmniCreator is the production truth.

Machine-readable rules are in `agent-harness/parallel-production/contract.json`.
