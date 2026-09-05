use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{Error, LogicalUri, Result};

pub const PRODUCTION_PACK_SCHEMA_V1: &str = "omnicreator.production-pack";
pub const PRODUCTION_PACK_VERSION_V1: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimelineFrameRateV1 {
    pub numerator: u32,
    pub denominator: u32,
}

impl TimelineFrameRateV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.numerator == 0 || self.denominator == 0 {
            return Err(Error::InvalidContract(
                "timeline frame rate numerator and denominator must be positive".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TimelineTrackRoleV1 {
    VideoBackground,
    VideoPrimary,
    VideoBroll,
    VideoGeneratedOverlay,
    VideoTypographyScripture,
    AudioNarration,
    AudioMusic,
    AudioAmbience,
    AudioSfx,
}

impl TimelineTrackRoleV1 {
    pub fn stable_order_v1(self) -> u8 {
        match self {
            Self::VideoBackground => 1,
            Self::VideoPrimary => 2,
            Self::VideoBroll => 3,
            Self::VideoGeneratedOverlay => 4,
            Self::VideoTypographyScripture => 5,
            Self::AudioNarration => 6,
            Self::AudioMusic => 7,
            Self::AudioAmbience => 8,
            Self::AudioSfx => 9,
        }
    }

    pub fn stable_name_v1(self) -> &'static str {
        match self {
            Self::VideoBackground => "V1 Background",
            Self::VideoPrimary => "V2 Primary Visual",
            Self::VideoBroll => "V3 B-roll",
            Self::VideoGeneratedOverlay => "V4 Generated Overlays",
            Self::VideoTypographyScripture => "V5 Typography / Scripture",
            Self::AudioNarration => "A1 Narration",
            Self::AudioMusic => "A2 Music",
            Self::AudioAmbience => "A3 Ambience",
            Self::AudioSfx => "A4 SFX",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimelineClipV1 {
    pub clip_id: String,
    pub artifact_id: String,
    pub uri: LogicalUri,
    pub timeline_start_ms: u64,
    pub source_start_ms: u64,
    pub duration_ms: u64,
    pub label: Option<String>,
}

impl TimelineClipV1 {
    pub fn validate_v1(&self) -> Result<()> {
        require_identifier_v1("timeline clip_id", &self.clip_id)?;
        require_identifier_v1("timeline artifact_id", &self.artifact_id)?;
        if self.duration_ms == 0 {
            return Err(Error::InvalidContract(format!(
                "timeline clip {} duration_ms must be positive",
                self.clip_id
            )));
        }
        validate_optional_text_v1("timeline clip label", self.label.as_deref())
    }

    pub fn timeline_end_ms_v1(&self) -> Result<u64> {
        self.timeline_start_ms
            .checked_add(self.duration_ms)
            .ok_or_else(|| {
                Error::InvalidContract(format!(
                    "timeline clip {} end time overflows u64 milliseconds",
                    self.clip_id
                ))
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimelineTrackV1 {
    pub role: TimelineTrackRoleV1,
    #[serde(default)]
    pub clips: Vec<TimelineClipV1>,
}

impl TimelineTrackV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_ordered_clips_v1(&self.clips)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TimelineMarkerKindV1 {
    Scene,
    Scripture,
    Review,
    Chapter,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimelineMarkerV1 {
    pub marker_id: String,
    pub position_ms: u64,
    pub label: String,
    pub kind: TimelineMarkerKindV1,
}

impl TimelineMarkerV1 {
    pub fn validate_v1(&self) -> Result<()> {
        require_identifier_v1("timeline marker_id", &self.marker_id)?;
        require_text_v1("timeline marker label", &self.label)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubtitleCueV1 {
    pub cue_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

impl SubtitleCueV1 {
    pub fn validate_v1(&self) -> Result<()> {
        require_identifier_v1("subtitle cue_id", &self.cue_id)?;
        if self.end_ms <= self.start_ms {
            return Err(Error::InvalidContract(format!(
                "subtitle cue {} end_ms must be greater than start_ms",
                self.cue_id
            )));
        }
        require_text_v1("subtitle cue text", &self.text)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProductionPackV1 {
    pub schema: String,
    pub version: u32,
    pub project_id: String,
    pub title: String,
    pub frame_rate: TimelineFrameRateV1,
    #[serde(default)]
    pub tracks: Vec<TimelineTrackV1>,
    #[serde(default)]
    pub subtitles: Vec<SubtitleCueV1>,
    #[serde(default)]
    pub markers: Vec<TimelineMarkerV1>,
}

impl ProductionPackV1 {
    pub fn normalized_v1(&self) -> Result<Self> {
        let mut normalized = self.clone();
        normalized
            .tracks
            .sort_by_key(|track| track.role.stable_order_v1());
        for track in &mut normalized.tracks {
            track.clips.sort_by(|left, right| {
                left.timeline_start_ms
                    .cmp(&right.timeline_start_ms)
                    .then_with(|| left.clip_id.cmp(&right.clip_id))
            });
        }
        normalized.subtitles.sort_by(|left, right| {
            left.start_ms
                .cmp(&right.start_ms)
                .then_with(|| left.end_ms.cmp(&right.end_ms))
                .then_with(|| left.cue_id.cmp(&right.cue_id))
        });
        normalized.markers.sort_by(|left, right| {
            left.position_ms
                .cmp(&right.position_ms)
                .then_with(|| left.marker_id.cmp(&right.marker_id))
        });
        normalized.validate_v1()?;
        Ok(normalized)
    }

    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != PRODUCTION_PACK_SCHEMA_V1 || self.version != PRODUCTION_PACK_VERSION_V1 {
            return Err(Error::InvalidContract(format!(
                "unsupported production pack contract {} v{}",
                self.schema, self.version
            )));
        }
        require_identifier_v1("production pack project_id", &self.project_id)?;
        require_text_v1("production pack title", &self.title)?;
        self.frame_rate.validate_v1()?;

        let mut roles = BTreeSet::new();
        let mut previous_order = 0;
        for track in &self.tracks {
            let order = track.role.stable_order_v1();
            if order < previous_order {
                return Err(Error::InvalidContract(
                    "production pack tracks are not in stable track-role order".to_owned(),
                ));
            }
            previous_order = order;
            if !roles.insert(track.role) {
                return Err(Error::InvalidContract(format!(
                    "production pack contains duplicate track role {}",
                    track.role.stable_name_v1()
                )));
            }
            track.validate_v1()?;
        }

        validate_ordered_subtitles_v1(&self.subtitles)?;

        let mut previous_marker: Option<(u64, &str)> = None;
        for marker in &self.markers {
            marker.validate_v1()?;
            if let Some((position_ms, marker_id)) = previous_marker {
                if (marker.position_ms, marker.marker_id.as_str()) < (position_ms, marker_id) {
                    return Err(Error::InvalidContract(
                        "production pack markers are not in deterministic order".to_owned(),
                    ));
                }
            }
            previous_marker = Some((marker.position_ms, marker.marker_id.as_str()));
        }

        Ok(())
    }

    pub fn render_srt_v1(&self) -> Result<String> {
        render_srt_v1(&self.subtitles)
    }
}

pub fn render_srt_v1(cues: &[SubtitleCueV1]) -> Result<String> {
    validate_ordered_subtitles_v1(cues)?;

    let mut rendered = String::new();
    for (index, cue) in cues.iter().enumerate() {
        if index != 0 {
            rendered.push('\n');
        }
        rendered.push_str(&(index + 1).to_string());
        rendered.push('\n');
        rendered.push_str(&format_srt_timestamp_v1(cue.start_ms));
        rendered.push_str(" --> ");
        rendered.push_str(&format_srt_timestamp_v1(cue.end_ms));
        rendered.push('\n');
        rendered.push_str(&normalize_srt_text_v1(&cue.text));
        rendered.push('\n');
    }
    Ok(rendered)
}

fn validate_ordered_clips_v1(clips: &[TimelineClipV1]) -> Result<()> {
    let mut previous_end_ms = 0;
    let mut seen = BTreeSet::new();
    for (index, clip) in clips.iter().enumerate() {
        clip.validate_v1()?;
        if !seen.insert(clip.clip_id.as_str()) {
            return Err(Error::InvalidContract(format!(
                "duplicate timeline clip_id {}",
                clip.clip_id
            )));
        }
        if index != 0 && clip.timeline_start_ms < previous_end_ms {
            return Err(Error::InvalidContract(format!(
                "timeline clip {} overlaps the previous clip on the same track",
                clip.clip_id
            )));
        }
        previous_end_ms = clip.timeline_end_ms_v1()?;
    }
    Ok(())
}

fn validate_ordered_subtitles_v1(cues: &[SubtitleCueV1]) -> Result<()> {
    let mut previous_end_ms = 0;
    let mut seen = BTreeSet::new();
    for (index, cue) in cues.iter().enumerate() {
        cue.validate_v1()?;
        if !seen.insert(cue.cue_id.as_str()) {
            return Err(Error::InvalidContract(format!(
                "duplicate subtitle cue_id {}",
                cue.cue_id
            )));
        }
        if index != 0 && cue.start_ms < previous_end_ms {
            return Err(Error::InvalidContract(format!(
                "subtitle cue {} overlaps or is out of order",
                cue.cue_id
            )));
        }
        previous_end_ms = cue.end_ms;
    }
    Ok(())
}

fn format_srt_timestamp_v1(milliseconds: u64) -> String {
    let total_seconds = milliseconds / 1000;
    let millis = milliseconds % 1000;
    let seconds = total_seconds % 60;
    let total_minutes = total_seconds / 60;
    let minutes = total_minutes % 60;
    let hours = total_minutes / 60;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

fn normalize_srt_text_v1(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_owned()
}

fn require_identifier_v1(label: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        return Err(Error::InvalidContract(format!(
            "{label} must be a non-empty identifier without surrounding whitespace or control characters"
        )));
    }
    Ok(())
}

fn require_text_v1(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') {
        return Err(Error::InvalidContract(format!(
            "{label} must contain non-empty text without NUL"
        )));
    }
    Ok(())
}

fn validate_optional_text_v1(label: &str, value: Option<&str>) -> Result<()> {
    if let Some(value) = value {
        require_text_v1(label, value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(id: &str, start_ms: u64, duration_ms: u64) -> TimelineClipV1 {
        TimelineClipV1 {
            clip_id: id.to_owned(),
            artifact_id: format!("artifact-{id}"),
            uri: LogicalUri::parse(&format!("project://visual/{id}.png")).unwrap(),
            timeline_start_ms: start_ms,
            source_start_ms: 0,
            duration_ms,
            label: None,
        }
    }

    #[test]
    fn production_pack_normalizes_stable_track_and_timing_order() {
        let pack = ProductionPackV1 {
            schema: PRODUCTION_PACK_SCHEMA_V1.to_owned(),
            version: PRODUCTION_PACK_VERSION_V1,
            project_id: "project-1".to_owned(),
            title: "Production Pack".to_owned(),
            frame_rate: TimelineFrameRateV1 {
                numerator: 24,
                denominator: 1,
            },
            tracks: vec![
                TimelineTrackV1 {
                    role: TimelineTrackRoleV1::AudioNarration,
                    clips: vec![clip("audio-2", 4_000, 2_000), clip("audio-1", 0, 4_000)],
                },
                TimelineTrackV1 {
                    role: TimelineTrackRoleV1::VideoPrimary,
                    clips: vec![clip("visual-1", 0, 6_000)],
                },
            ],
            subtitles: vec![
                SubtitleCueV1 {
                    cue_id: "S02".to_owned(),
                    start_ms: 3_000,
                    end_ms: 5_000,
                    text: "Second".to_owned(),
                },
                SubtitleCueV1 {
                    cue_id: "S01".to_owned(),
                    start_ms: 0,
                    end_ms: 3_000,
                    text: "First".to_owned(),
                },
            ],
            markers: vec![
                TimelineMarkerV1 {
                    marker_id: "M02".to_owned(),
                    position_ms: 3_000,
                    label: "Second scene".to_owned(),
                    kind: TimelineMarkerKindV1::Scene,
                },
                TimelineMarkerV1 {
                    marker_id: "M01".to_owned(),
                    position_ms: 0,
                    label: "Opening".to_owned(),
                    kind: TimelineMarkerKindV1::Chapter,
                },
            ],
        };

        let normalized = pack.normalized_v1().unwrap();
        assert_eq!(normalized.tracks[0].role, TimelineTrackRoleV1::VideoPrimary);
        assert_eq!(
            normalized.tracks[1].role,
            TimelineTrackRoleV1::AudioNarration
        );
        assert_eq!(normalized.tracks[1].clips[0].clip_id, "audio-1");
        assert_eq!(normalized.subtitles[0].cue_id, "S01");
        assert_eq!(normalized.markers[0].marker_id, "M01");
    }

    #[test]
    fn srt_render_is_deterministic_and_uses_millisecond_timestamps() {
        let cues = vec![
            SubtitleCueV1 {
                cue_id: "S01".to_owned(),
                start_ms: 0,
                end_ms: 1_234,
                text: "First line\r\nsecond line".to_owned(),
            },
            SubtitleCueV1 {
                cue_id: "S02".to_owned(),
                start_ms: 61_001,
                end_ms: 3_661_002,
                text: "Long cue".to_owned(),
            },
        ];

        assert_eq!(
            render_srt_v1(&cues).unwrap(),
            "1\n00:00:00,000 --> 00:00:01,234\nFirst line\nsecond line\n\n2\n00:01:01,001 --> 01:01:01,002\nLong cue\n"
        );
    }

    #[test]
    fn overlapping_subtitles_and_track_clips_are_rejected() {
        let subtitles = vec![
            SubtitleCueV1 {
                cue_id: "S01".to_owned(),
                start_ms: 0,
                end_ms: 2_000,
                text: "One".to_owned(),
            },
            SubtitleCueV1 {
                cue_id: "S02".to_owned(),
                start_ms: 1_999,
                end_ms: 3_000,
                text: "Two".to_owned(),
            },
        ];
        assert!(matches!(
            render_srt_v1(&subtitles),
            Err(Error::InvalidContract(_))
        ));

        let track = TimelineTrackV1 {
            role: TimelineTrackRoleV1::VideoPrimary,
            clips: vec![clip("one", 0, 2_000), clip("two", 1_999, 1_000)],
        };
        assert!(matches!(
            track.validate_v1(),
            Err(Error::InvalidContract(_))
        ));
    }

    #[test]
    fn serialized_pack_contains_only_portable_logical_media_references() {
        let root = "/Users/example/Google Drive/OmniCreatorData";
        let pack = ProductionPackV1 {
            schema: PRODUCTION_PACK_SCHEMA_V1.to_owned(),
            version: PRODUCTION_PACK_VERSION_V1,
            project_id: "project-1".to_owned(),
            title: "Portable Pack".to_owned(),
            frame_rate: TimelineFrameRateV1 {
                numerator: 30_000,
                denominator: 1_001,
            },
            tracks: vec![TimelineTrackV1 {
                role: TimelineTrackRoleV1::VideoPrimary,
                clips: vec![clip("SC01", 0, 5_000)],
            }],
            subtitles: vec![],
            markers: vec![],
        }
        .normalized_v1()
        .unwrap();

        let json = serde_json::to_string_pretty(&pack).unwrap();
        assert!(json.contains("project://visual/SC01.png"));
        assert!(!json.contains(root));
        assert!(!json.contains("C:\\\\"));
        assert!(!json.contains("/home/"));
    }
}
