use std::fs;

use omnicreator_core::{
    compile_creator_workflow_plan_v1, creator_segment_voice_states_v1,
    derive_manual_voice_timing_v1, initial_studio_pack_catalog_v1, inspect_manual_voice_audio_v1,
    load_latest_creator_content_v1, materialize_creator_workflow_plan_v1,
    parse_manual_voice_timing_bytes_v1,
    provide_manual_creator_content_v1, provide_manual_creator_voice_bundle_v1,
    replace_manual_creator_voice_timing_v1, voice_timing_to_srt_v1, ArtifactStore,
    ManualCreatorVoiceRequestV1, ManualResultProvenanceV1, StateStore, StepStatus,
    VoiceTimingCueV1, VoiceTimingV1, Workspace, CREATOR_STEP_PRODUCTION_PACK_V1,
    CREATOR_STEP_VOICE_PREPARE_V1, CREATOR_WORKFLOW_UNIT_PROJECT_V1, VOICE_TIMING_SCHEMA_V1,
};

struct Fixture {
    temp: tempfile::TempDir,
    workspace: Workspace,
    store: StateStore,
    project_id: String,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::create(temp.path().join("data")).unwrap();
    let store = StateStore::open(workspace.sqlite_path()).unwrap();
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let project = store
        .create_project_with_studio_pack("Manual voice", Some(&pack.id))
        .unwrap();
    let plan = compile_creator_workflow_plan_v1(&project, &pack).unwrap();
    materialize_creator_workflow_plan_v1(&store, &plan).unwrap();
    Fixture {
        temp,
        workspace,
        store,
        project_id: project.id,
    }
}

fn prepare_content(fx: &mut Fixture) {
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();
    provide_manual_creator_content_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        "First voice segment.\n\nSecond voice segment.",
        ManualResultProvenanceV1::manual_editor(),
    )
    .unwrap();
}

fn content_segments(fx: &Fixture) -> Vec<(String, String)> {
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();
    let (content, _) = load_latest_creator_content_v1(&fx.store, &artifacts, &fx.project_id)
        .unwrap()
        .unwrap();
    content
        .segments
        .into_iter()
        .map(|segment| (segment.id, segment.narration))
        .collect()
}

fn wav(duration_ms: u64, marker: u8) -> Vec<u8> {
    let sample_rate = 8_000u32;
    let channels = 1u16;
    let bits = 16u16;
    let block_align = channels * (bits / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let data_size = ((u64::from(byte_rate) * duration_ms) / 1_000) as u32;
    let riff_size = 36u32 + data_size;
    let mut out = Vec::with_capacity((44 + data_size) as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend(std::iter::repeat(marker).take(data_size as usize));
    out
}

fn step(store: &StateStore, project_id: &str, key: &str) -> omnicreator_core::WorkflowStep {
    store
        .list_project_steps(project_id)
        .unwrap()
        .into_iter()
        .find(|step| step.step == key && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1)
        .unwrap()
}

#[test]
fn no_voice_provider_or_gpu_manual_wav_segments_complete_voice_stage() {
    let mut fx = fixture();
    prepare_content(&mut fx);
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();

    for (index, (segment_id, narration)) in content_segments(&fx).into_iter().enumerate() {
        let audio = fx.temp.path().join(format!("{segment_id}.wav"));
        fs::write(&audio, wav(1_000, index as u8 + 1)).unwrap();
        let timing =
            derive_manual_voice_timing_v1(&segment_id, &narration, 1_000).unwrap();
        let outcome = provide_manual_creator_voice_bundle_v1(
            &mut fx.store,
            &artifacts,
            ManualCreatorVoiceRequestV1 {
                project_id: &fx.project_id,
                segment_id: &segment_id,
                audio_path: &audio,
                timing,
                provenance: ManualResultProvenanceV1::local_file(),
                replace_existing: false,
            },
        )
        .unwrap();
        assert!(artifacts.verify_artifact(&outcome.audio).unwrap());
        assert!(artifacts.verify_artifact(&outcome.timing_artifact).unwrap());
        if index == 0 {
            assert!(!outcome.voice_stage_complete);
        } else {
            assert!(outcome.voice_stage_complete);
        }
    }

    assert_eq!(
        step(&fx.store, &fx.project_id, CREATOR_STEP_VOICE_PREPARE_V1).status,
        StepStatus::Succeeded
    );
    let states = creator_segment_voice_states_v1(&fx.store, &artifacts, &fx.project_id).unwrap();
    assert_eq!(states.len(), 2);
    assert!(states.iter().all(|state| state.verified));
}

#[test]
fn srt_import_and_round_trip_validate_order_duration_and_linkage() {
    let valid =
        b"1\n00:00:00,000 --> 00:00:00,400\nFirst\n\n2\n00:00:00,400 --> 00:00:01,000\nSecond\n";
    let timing = parse_manual_voice_timing_bytes_v1(valid, "srt", "SEG001", Some(1_000)).unwrap();
    assert_eq!(timing.duration_ms, 1_000);
    assert_eq!(timing.cues.len(), 2);
    let rendered = voice_timing_to_srt_v1(&timing).unwrap();
    assert!(rendered.contains("00:00:00,400 --> 00:00:01,000"));

    let overlap = b"1\n00:00:00,000 --> 00:00:00,700\nA\n\n2\n00:00:00,600 --> 00:00:01,000\nB\n";
    assert!(parse_manual_voice_timing_bytes_v1(overlap, "srt", "SEG001", Some(1_000)).is_err());

    let mismatched = VoiceTimingV1 {
        schema: VOICE_TIMING_SCHEMA_V1.to_owned(),
        version: 1,
        segment_id: "SEG999".to_owned(),
        duration_ms: 1_000,
        cues: vec![VoiceTimingCueV1 {
            index: 0,
            text: "x".to_owned(),
            start_ms: 0,
            end_ms: 1_000,
        }],
    };
    let json = mismatched.to_json_bytes_v1().unwrap();
    assert!(parse_manual_voice_timing_bytes_v1(&json, "json", "SEG001", Some(1_000)).is_err());
}

#[test]
fn timing_replacement_reuses_audio_and_preserves_other_segment_work() {
    let mut fx = fixture();
    prepare_content(&mut fx);
    let artifacts = ArtifactStore::new(fx.workspace.data_root()).unwrap();

    let segments = content_segments(&fx);
    let mut original_jobs = Vec::new();
    for (index, (segment_id, narration)) in segments.iter().enumerate() {
        let audio = fx.temp.path().join(format!("replace-{segment_id}.wav"));
        fs::write(&audio, wav(1_000, index as u8 + 3)).unwrap();
        let timing =
            derive_manual_voice_timing_v1(segment_id, narration, 1_000).unwrap();
        let outcome = provide_manual_creator_voice_bundle_v1(
            &mut fx.store,
            &artifacts,
            ManualCreatorVoiceRequestV1 {
                project_id: &fx.project_id,
                segment_id,
                audio_path: &audio,
                timing,
                provenance: ManualResultProvenanceV1::local_file(),
                replace_existing: false,
            },
        )
        .unwrap();
        original_jobs.push((
            segment_id.to_owned(),
            outcome.job.job_id,
            outcome.audio.sha256,
        ));
    }
    let production = step(&fx.store, &fx.project_id, CREATOR_STEP_PRODUCTION_PACK_V1);
    assert_eq!(production.status, StepStatus::Ready);
    fx.store
        .set_step_status(&production.step_id, StepStatus::Succeeded)
        .unwrap();

    let corrected = VoiceTimingV1 {
        schema: VOICE_TIMING_SCHEMA_V1.to_owned(),
        version: 1,
        segment_id: segments[0].0.clone(),
        duration_ms: 1_000,
        cues: vec![
            VoiceTimingCueV1 {
                index: 0,
                text: "First half".to_owned(),
                start_ms: 0,
                end_ms: 500,
            },
            VoiceTimingCueV1 {
                index: 1,
                text: "Second half".to_owned(),
                start_ms: 500,
                end_ms: 1_000,
            },
        ],
    };
    let replacement = replace_manual_creator_voice_timing_v1(
        &mut fx.store,
        &artifacts,
        &fx.project_id,
        &segments[0].0,
        corrected,
        ManualResultProvenanceV1::manual_editor(),
    )
    .unwrap();

    assert!(replacement.voice_stage_complete);
    assert_eq!(replacement.audio.sha256, original_jobs[0].2);
    assert_eq!(
        step(&fx.store, &fx.project_id, CREATOR_STEP_VOICE_PREPARE_V1).status,
        StepStatus::Succeeded
    );
    assert_eq!(
        step(&fx.store, &fx.project_id, CREATOR_STEP_PRODUCTION_PACK_V1).status,
        StepStatus::Ready
    );
    let jobs = fx.store.list_project_jobs(&fx.project_id).unwrap();
    assert_eq!(
        jobs.iter()
            .find(|job| job.job_id == original_jobs[0].1)
            .unwrap()
            .status,
        StepStatus::Stale
    );
    assert_eq!(
        jobs.iter()
            .find(|job| job.job_id == original_jobs[1].1)
            .unwrap()
            .status,
        StepStatus::Succeeded
    );
}

#[test]
fn audio_inspection_rejects_spoofing_and_reports_wav_duration() {
    let fx = fixture();
    let good = fx.temp.path().join("good.wav");
    fs::write(&good, wav(1_250, 9)).unwrap();
    let metadata = inspect_manual_voice_audio_v1(&good).unwrap();
    assert_eq!(metadata.duration_ms, Some(1_250));

    let fake = fx.temp.path().join("fake.wav");
    fs::write(&fake, b"not wave").unwrap();
    assert!(inspect_manual_voice_audio_v1(&fake).is_err());
}
