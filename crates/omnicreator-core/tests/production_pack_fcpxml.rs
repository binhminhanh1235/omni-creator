use std::fs;

use omnicreator_core::{
    deterministic_input_hash, Artifact, ArtifactStore, Error, FcpxmlExporterV1, LogicalUri,
    ProductionPackV1, StateStore, TimelineClipV1, TimelineFrameRateV1, TimelineMarkerKindV1,
    TimelineMarkerV1, TimelineTrackRoleV1, TimelineTrackV1, Workspace, PRODUCTION_PACK_SCHEMA_V1,
    PRODUCTION_PACK_VERSION_V1,
};

struct Fixture {
    _temp: tempfile::TempDir,
    state: StateStore,
    artifacts: ArtifactStore,
    project_id: String,
    counter: usize,
}

impl Fixture {
    fn new(root_name: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join(root_name);
        let workspace = Workspace::create(&root).unwrap();
        let state = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = state.create_project("Export & <Project>").unwrap();
        let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
        Self {
            _temp: temp,
            state,
            artifacts,
            project_id: project.id,
            counter: 0,
        }
    }

    fn promote(&mut self, uri: &str, artifact_type: &str, bytes: &[u8]) -> Artifact {
        self.counter += 1;
        let input_hash = deterministic_input_hash(&[
            b"phase9-p1",
            self.counter.to_string().as_bytes(),
            uri.as_bytes(),
            bytes,
        ]);
        let job = self
            .state
            .create_job(
                &self.project_id,
                "production-pack-source",
                &format!("asset-{}", self.counter),
                &input_hash,
            )
            .unwrap();
        let source = self
            ._temp
            .path()
            .join(format!("source-{}.bin", self.counter));
        fs::write(&source, bytes).unwrap();
        self.artifacts
            .promote_job_output(
                &mut self.state,
                &job.job_id,
                &source,
                LogicalUri::parse(uri).unwrap(),
                artifact_type,
                serde_json::json!({"source": "test"}),
            )
            .unwrap()
    }
}

fn clip(
    id: &str,
    artifact: &Artifact,
    timeline_start_ms: u64,
    source_start_ms: u64,
    duration_ms: u64,
    label: Option<&str>,
) -> TimelineClipV1 {
    TimelineClipV1 {
        clip_id: id.to_owned(),
        artifact_id: artifact.artifact_id.clone(),
        uri: artifact.uri.clone(),
        timeline_start_ms,
        source_start_ms,
        duration_ms,
        label: label.map(str::to_owned),
    }
}

fn pack(
    project_id: &str,
    tracks: Vec<TimelineTrackV1>,
    markers: Vec<TimelineMarkerV1>,
) -> ProductionPackV1 {
    ProductionPackV1 {
        schema: PRODUCTION_PACK_SCHEMA_V1.to_owned(),
        version: PRODUCTION_PACK_VERSION_V1,
        project_id: project_id.to_owned(),
        title: "Export & <Project> \"Quoted\"".to_owned(),
        frame_rate: TimelineFrameRateV1 {
            numerator: 24,
            denominator: 1,
        },
        tracks,
        subtitles: vec![],
        markers,
    }
}

#[test]
fn one_video_and_narration_produce_deterministic_interchange() {
    let mut fixture = Fixture::new("OmniCreatorData");
    let video = fixture.promote("project://video/scene-01.mp4", "video", b"video");
    let narration = fixture.promote("project://audio/narration.wav", "audio", b"audio");
    let production_pack = pack(
        &fixture.project_id,
        vec![
            TimelineTrackV1 {
                role: TimelineTrackRoleV1::AudioNarration,
                clips: vec![clip("A01", &narration, 0, 0, 4_000, Some("Narration"))],
            },
            TimelineTrackV1 {
                role: TimelineTrackRoleV1::VideoPrimary,
                clips: vec![clip("V01", &video, 0, 0, 4_000, Some("Primary"))],
            },
        ],
        vec![],
    );

    let exporter = FcpxmlExporterV1::default();
    let first = exporter
        .render(&fixture.state, &fixture.artifacts, &production_pack)
        .unwrap();
    let second = exporter
        .render(&fixture.state, &fixture.artifacts, &production_pack)
        .unwrap();

    assert_eq!(first, second);
    assert!(
        first.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<fcpxml version=\"1.10\">")
    );
    assert!(first.contains("<event name=\"Export &amp; &lt;Project&gt; &quot;Quoted&quot;\">"));
    assert!(first.contains("videoRole=\"video.primary\""));
    assert!(first.contains("audioRole=\"dialogue.narration\""));
    assert!(first.contains("kind=\"original-media\""));
    assert!(!first.contains("proxy-media"));
}

#[test]
fn multiple_roles_preserve_stable_deterministic_lane_layout() {
    let mut fixture = Fixture::new("OmniCreatorData");
    let mut tracks = Vec::new();
    let roles = [
        (TimelineTrackRoleV1::VideoBackground, 1, "video.background"),
        (TimelineTrackRoleV1::VideoPrimary, 2, "video.primary"),
        (TimelineTrackRoleV1::VideoBroll, 3, "video.broll"),
        (
            TimelineTrackRoleV1::VideoGeneratedOverlay,
            4,
            "video.generated-overlays",
        ),
        (
            TimelineTrackRoleV1::VideoTypographyScripture,
            5,
            "video.typography-scripture",
        ),
        (
            TimelineTrackRoleV1::AudioNarration,
            -1,
            "dialogue.narration",
        ),
        (TimelineTrackRoleV1::AudioMusic, -2, "music.music"),
        (TimelineTrackRoleV1::AudioAmbience, -3, "effects.ambience"),
        (TimelineTrackRoleV1::AudioSfx, -4, "effects.sfx"),
    ];

    for (index, (role, _, _)) in roles.iter().enumerate().rev() {
        let media = fixture.promote(
            &format!("project://media/role-{index}.bin"),
            if matches!(
                *role,
                TimelineTrackRoleV1::AudioNarration
                    | TimelineTrackRoleV1::AudioMusic
                    | TimelineTrackRoleV1::AudioAmbience
                    | TimelineTrackRoleV1::AudioSfx
            ) {
                "audio"
            } else {
                "video"
            },
            format!("media-{index}").as_bytes(),
        );
        tracks.push(TimelineTrackV1 {
            role: *role,
            clips: vec![clip(&format!("clip-{index}"), &media, 0, 0, 1_000, None)],
        });
    }

    let production_pack = pack(&fixture.project_id, tracks, vec![]);
    let xml = FcpxmlExporterV1::default()
        .render(&fixture.state, &fixture.artifacts, &production_pack)
        .unwrap();

    let mut last_index = 0;
    for (role, lane, role_name) in roles {
        let stable_name = role.stable_name_v1();
        let index = xml.find(stable_name).unwrap();
        assert!(
            index >= last_index,
            "{stable_name} must remain in stable order"
        );
        last_index = index;
        assert!(xml.contains(&format!("lane=\"{lane}\"")));
        assert!(xml.contains(role_name));
    }
}

#[test]
fn clip_timeline_source_and_duration_are_serialized_exactly() {
    let mut fixture = Fixture::new("OmniCreatorData");
    let video = fixture.promote("project://video/trimmed.mp4", "video", b"video-trim");
    let production_pack = pack(
        &fixture.project_id,
        vec![TimelineTrackV1 {
            role: TimelineTrackRoleV1::VideoBroll,
            clips: vec![clip("trim", &video, 1_250, 2_500, 3_750, None)],
        }],
        vec![],
    );
    let xml = FcpxmlExporterV1::default()
        .render(&fixture.state, &fixture.artifacts, &production_pack)
        .unwrap();

    assert!(xml.contains("offset=\"1250/1000s\" start=\"2500/1000s\" duration=\"3750/1000s\""));
    assert!(xml.contains("duration=\"6250/1000s\""));
}

#[test]
fn markers_and_xml_reserved_characters_are_exported_safely() {
    let mut fixture = Fixture::new("OmniCreatorData");
    let video = fixture.promote("project://video/marker.mp4", "video", b"marker-video");
    let production_pack = pack(
        &fixture.project_id,
        vec![TimelineTrackV1 {
            role: TimelineTrackRoleV1::VideoPrimary,
            clips: vec![clip(
                "V<&>\"'",
                &video,
                0,
                0,
                2_000,
                Some("Clip & < > \" '"),
            )],
        }],
        vec![TimelineMarkerV1 {
            marker_id: "M01".to_owned(),
            position_ms: 1_500,
            label: "Review & <fix> \"quote\"".to_owned(),
            kind: TimelineMarkerKindV1::Review,
        }],
    );
    let xml = FcpxmlExporterV1::default()
        .render(&fixture.state, &fixture.artifacts, &production_pack)
        .unwrap();

    assert!(xml.contains("Clip &amp; &lt; &gt; &quot; &apos;"));
    assert!(xml.contains("value=\"Review | Review &amp; &lt;fix&gt; &quot;quote&quot;\""));
    assert!(xml.contains("start=\"1500/1000s\" duration=\"1/24s\""));
}

#[test]
fn file_urls_escape_spaces_unicode_hash_and_percent() {
    let mut fixture = Fixture::new("Omni Creator Data");
    let media = fixture.promote(
        "project://video/ảnh scene #100%.mp4",
        "video",
        b"special-path",
    );
    let production_pack = pack(
        &fixture.project_id,
        vec![TimelineTrackV1 {
            role: TimelineTrackRoleV1::VideoPrimary,
            clips: vec![clip("special", &media, 0, 0, 1_000, None)],
        }],
        vec![],
    );
    let xml = FcpxmlExporterV1::default()
        .render(&fixture.state, &fixture.artifacts, &production_pack)
        .unwrap();

    assert!(xml.contains("Omni%20Creator%20Data"));
    assert!(xml.contains("%E1%BA%A3nh%20scene%20%23100%25.mp4"));
    assert!(!xml.contains("ảnh scene #100%.mp4"));
}

#[test]
fn missing_artifact_returns_typed_not_found_error() {
    let fixture = Fixture::new("OmniCreatorData");
    let fake = Artifact {
        artifact_id: "art_missing".to_owned(),
        project_id: Some(fixture.project_id.clone()),
        artifact_type: "video".to_owned(),
        uri: LogicalUri::parse("project://video/missing.mp4").unwrap(),
        sha256: "00".to_owned(),
        size_bytes: 1,
        input_hash: None,
        producer_job: None,
        created_at: chrono::Utc::now(),
        metadata: serde_json::Value::Null,
    };
    let production_pack = pack(
        &fixture.project_id,
        vec![TimelineTrackV1 {
            role: TimelineTrackRoleV1::VideoPrimary,
            clips: vec![clip("missing", &fake, 0, 0, 1_000, None)],
        }],
        vec![],
    );

    assert!(matches!(
        FcpxmlExporterV1::default().render(
            &fixture.state,
            &fixture.artifacts,
            &production_pack
        ),
        Err(Error::ArtifactNotFound(id)) if id == "art_missing"
    ));
}

#[test]
fn logical_uri_mismatch_is_rejected_before_serialization() {
    let mut fixture = Fixture::new("OmniCreatorData");
    let media = fixture.promote("project://video/right.mp4", "video", b"right");
    let mut wrong = clip("wrong-uri", &media, 0, 0, 1_000, None);
    wrong.uri = LogicalUri::parse("project://video/wrong.mp4").unwrap();
    let production_pack = pack(
        &fixture.project_id,
        vec![TimelineTrackV1 {
            role: TimelineTrackRoleV1::VideoPrimary,
            clips: vec![wrong],
        }],
        vec![],
    );

    assert!(matches!(
        FcpxmlExporterV1::default().render(
            &fixture.state,
            &fixture.artifacts,
            &production_pack
        ),
        Err(Error::ExportArtifactUriMismatch { artifact_id, .. })
            if artifact_id == media.artifact_id
    ));
}

#[test]
fn artifact_from_another_project_is_rejected() {
    let mut fixture = Fixture::new("OmniCreatorData");
    let media = fixture.promote("project://video/project-a.mp4", "video", b"a");
    let other = fixture.state.create_project("Other").unwrap();
    let production_pack = pack(
        &other.id,
        vec![TimelineTrackV1 {
            role: TimelineTrackRoleV1::VideoPrimary,
            clips: vec![clip("cross-project", &media, 0, 0, 1_000, None)],
        }],
        vec![],
    );

    assert!(matches!(
        FcpxmlExporterV1::default().render(
            &fixture.state,
            &fixture.artifacts,
            &production_pack
        ),
        Err(Error::ExportArtifactProjectMismatch { artifact_id, .. })
            if artifact_id == media.artifact_id
    ));
}

#[test]
fn missing_physical_file_fails_before_export_success() {
    let mut fixture = Fixture::new("OmniCreatorData");
    let media = fixture.promote("project://video/gone.mp4", "video", b"gone");
    let path = fixture.artifacts.resolve_artifact_path(&media).unwrap();
    fs::remove_file(&path).unwrap();
    let production_pack = pack(
        &fixture.project_id,
        vec![TimelineTrackV1 {
            role: TimelineTrackRoleV1::VideoPrimary,
            clips: vec![clip("gone", &media, 0, 0, 1_000, None)],
        }],
        vec![],
    );

    assert!(matches!(
        FcpxmlExporterV1::default().render(
            &fixture.state,
            &fixture.artifacts,
            &production_pack
        ),
        Err(Error::ExportArtifactFileMissing { artifact_id, .. })
            if artifact_id == media.artifact_id
    ));
}

#[test]
fn corrupt_physical_file_fails_hash_verification() {
    let mut fixture = Fixture::new("OmniCreatorData");
    let media = fixture.promote("project://video/corrupt.mp4", "video", b"valid");
    let path = fixture.artifacts.resolve_artifact_path(&media).unwrap();
    fs::write(&path, b"corrupt").unwrap();
    let production_pack = pack(
        &fixture.project_id,
        vec![TimelineTrackV1 {
            role: TimelineTrackRoleV1::VideoPrimary,
            clips: vec![clip("corrupt", &media, 0, 0, 1_000, None)],
        }],
        vec![],
    );

    assert!(matches!(
        FcpxmlExporterV1::default().render(
            &fixture.state,
            &fixture.artifacts,
            &production_pack
        ),
        Err(Error::ArtifactHashMismatch(id)) if id == media.artifact_id
    ));
}

#[test]
fn data_root_move_regenerates_new_file_urls_without_changing_canonical_pack() {
    let temp = tempfile::tempdir().unwrap();
    let old_root = temp.path().join("old path").join("OmniCreatorData");
    let new_root = temp.path().join("new path").join("OmniCreatorData");

    let workspace = Workspace::create(&old_root).unwrap();
    let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = state.create_project("Portable Project").unwrap();
    let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
    let job = state
        .create_job(
            &project.id,
            "source",
            "V01",
            &deterministic_input_hash(&[b"portable"]),
        )
        .unwrap();
    let source = temp.path().join("portable.bin");
    fs::write(&source, b"portable-media").unwrap();
    let media = artifacts
        .promote_job_output(
            &mut state,
            &job.job_id,
            &source,
            LogicalUri::parse("project://video/portable scene.mp4").unwrap(),
            "video",
            serde_json::Value::Null,
        )
        .unwrap();

    let production_pack = pack(
        &project.id,
        vec![TimelineTrackV1 {
            role: TimelineTrackRoleV1::VideoPrimary,
            clips: vec![clip("portable", &media, 0, 0, 2_000, None)],
        }],
        vec![],
    )
    .normalized_v1()
    .unwrap();
    let canonical_before = serde_json::to_string(&production_pack).unwrap();
    let xml_a = FcpxmlExporterV1::default()
        .render(&state, &artifacts, &production_pack)
        .unwrap();
    let old_url_fragment = old_root
        .to_string_lossy()
        .replace('\\', "/")
        .replace(' ', "%20");
    assert!(xml_a.contains(&old_url_fragment));

    drop(state);
    drop(artifacts);
    drop(workspace);
    fs::create_dir_all(new_root.parent().unwrap()).unwrap();
    fs::rename(&old_root, &new_root).unwrap();

    let moved_workspace = Workspace::open(&new_root).unwrap();
    let moved_state = StateStore::open(moved_workspace.sqlite_path()).unwrap();
    let moved_artifacts = ArtifactStore::new(moved_workspace.data_root()).unwrap();
    let xml_b = FcpxmlExporterV1::default()
        .render(&moved_state, &moved_artifacts, &production_pack)
        .unwrap();
    let new_url_fragment = new_root
        .to_string_lossy()
        .replace('\\', "/")
        .replace(' ', "%20");

    assert!(xml_b.contains(&new_url_fragment));
    assert!(!xml_b.contains(&old_url_fragment));
    assert_eq!(
        serde_json::to_string(&production_pack).unwrap(),
        canonical_before
    );
    assert!(!canonical_before.contains(old_root.to_string_lossy().as_ref()));
    assert!(!canonical_before.contains(new_root.to_string_lossy().as_ref()));
}

#[test]
fn exporter_does_not_mutate_portable_production_pack() {
    let mut fixture = Fixture::new("OmniCreatorData");
    let first = fixture.promote("project://video/first.mp4", "video", b"first");
    let second = fixture.promote("project://video/second.mp4", "video", b"second");
    let production_pack = pack(
        &fixture.project_id,
        vec![TimelineTrackV1 {
            role: TimelineTrackRoleV1::VideoPrimary,
            clips: vec![
                clip("second", &second, 1_000, 0, 1_000, None),
                clip("first", &first, 0, 0, 1_000, None),
            ],
        }],
        vec![],
    );
    let before = production_pack.clone();

    let _ = FcpxmlExporterV1::default()
        .render(&fixture.state, &fixture.artifacts, &production_pack)
        .unwrap();

    assert_eq!(production_pack, before);
}

#[test]
fn same_normalized_inputs_and_data_root_produce_byte_identical_output() {
    let mut fixture = Fixture::new("OmniCreatorData");
    let media = fixture.promote("project://video/deterministic.mp4", "video", b"same");
    let unordered = pack(
        &fixture.project_id,
        vec![TimelineTrackV1 {
            role: TimelineTrackRoleV1::VideoPrimary,
            clips: vec![clip("deterministic", &media, 0, 125, 875, None)],
        }],
        vec![TimelineMarkerV1 {
            marker_id: "M01".to_owned(),
            position_ms: 500,
            label: "Middle".to_owned(),
            kind: TimelineMarkerKindV1::Scene,
        }],
    );
    let normalized = unordered.normalized_v1().unwrap();
    let exporter = FcpxmlExporterV1::default();

    let a = exporter
        .render(&fixture.state, &fixture.artifacts, &normalized)
        .unwrap();
    let b = exporter
        .render(&fixture.state, &fixture.artifacts, &normalized)
        .unwrap();
    assert_eq!(a.as_bytes(), b.as_bytes());
}
