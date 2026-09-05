use std::{collections::BTreeMap, path::Path};

use crate::{
    ArtifactStore, Error, LogicalUri, ProductionPackV1, Result, StateStore, TimelineMarkerKindV1,
    TimelineTrackRoleV1,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FcpxmlCompatibilityProfileV1 {
    Fcpxml110DaVinci,
}

impl FcpxmlCompatibilityProfileV1 {
    pub fn version(self) -> &'static str {
        match self {
            Self::Fcpxml110DaVinci => "1.10",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FcpxmlExportProfileV1 {
    pub compatibility: FcpxmlCompatibilityProfileV1,
}

impl Default for FcpxmlExportProfileV1 {
    fn default() -> Self {
        Self {
            compatibility: FcpxmlCompatibilityProfileV1::Fcpxml110DaVinci,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FcpxmlExporterV1 {
    profile: FcpxmlExportProfileV1,
}

impl FcpxmlExporterV1 {
    pub fn new(profile: FcpxmlExportProfileV1) -> Self {
        Self { profile }
    }

    pub fn profile(&self) -> FcpxmlExportProfileV1 {
        self.profile
    }

    pub fn render(
        &self,
        state_store: &StateStore,
        artifact_store: &ArtifactStore,
        production_pack: &ProductionPackV1,
    ) -> Result<String> {
        let normalized = production_pack.normalized_v1()?;
        let resolved = resolve_interchange_input(state_store, artifact_store, &normalized)?;
        render_fcpxml(self.profile, &normalized, &resolved)
    }
}

impl Default for FcpxmlExporterV1 {
    fn default() -> Self {
        Self::new(FcpxmlExportProfileV1::default())
    }
}

#[derive(Debug, Clone)]
struct ResolvedAsset {
    artifact_id: String,
    uri: LogicalUri,
    file_url: String,
    source_duration_ms: u64,
    has_video: bool,
    has_audio: bool,
}

#[derive(Debug, Clone)]
struct ResolvedClip {
    artifact_id: String,
    clip_id: String,
    track_role: TimelineTrackRoleV1,
    timeline_start_ms: u64,
    source_start_ms: u64,
    duration_ms: u64,
    label: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedInterchangeInput {
    assets: BTreeMap<String, ResolvedAsset>,
    clips: Vec<ResolvedClip>,
}

fn resolve_interchange_input(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    production_pack: &ProductionPackV1,
) -> Result<ResolvedInterchangeInput> {
    let mut assets = BTreeMap::new();
    let mut clips = Vec::new();

    for track in &production_pack.tracks {
        for clip in &track.clips {
            let artifact = state_store.get_artifact(&clip.artifact_id)?;

            if artifact.project_id.as_deref() != Some(production_pack.project_id.as_str()) {
                return Err(Error::ExportArtifactProjectMismatch {
                    artifact_id: artifact.artifact_id,
                    expected_project: production_pack.project_id.clone(),
                    actual_project: artifact.project_id,
                });
            }
            if artifact.uri != clip.uri {
                return Err(Error::ExportArtifactUriMismatch {
                    artifact_id: clip.artifact_id.clone(),
                    expected_uri: clip.uri.to_string(),
                    actual_uri: artifact.uri.to_string(),
                });
            }

            let path = artifact_store.resolve_artifact_path(&artifact)?;
            if !path.is_file() {
                return Err(Error::ExportArtifactFileMissing {
                    artifact_id: artifact.artifact_id,
                    path,
                });
            }
            if !artifact_store.verify_artifact(&artifact)? {
                return Err(Error::ExportArtifactFileMissing {
                    artifact_id: artifact.artifact_id,
                    path,
                });
            }

            let source_duration_ms = clip
                .source_start_ms
                .checked_add(clip.duration_ms)
                .ok_or_else(|| {
                    Error::InvalidContract(format!(
                        "timeline clip {} source range overflows u64 milliseconds",
                        clip.clip_id
                    ))
                })?;
            let file_url = file_url_from_absolute_path(&path)?;

            let entry = assets
                .entry(artifact.artifact_id.clone())
                .or_insert_with(|| ResolvedAsset {
                    artifact_id: artifact.artifact_id.clone(),
                    uri: artifact.uri.clone(),
                    file_url: file_url.clone(),
                    source_duration_ms,
                    has_video: false,
                    has_audio: false,
                });

            if entry.uri != artifact.uri || entry.file_url != file_url {
                return Err(Error::InvalidArtifact(format!(
                    "artifact {} resolved inconsistently during interchange export",
                    artifact.artifact_id
                )));
            }
            entry.source_duration_ms = entry.source_duration_ms.max(source_duration_ms);
            if is_video_role(track.role) {
                entry.has_video = true;
            } else {
                entry.has_audio = true;
            }

            clips.push(ResolvedClip {
                artifact_id: clip.artifact_id.clone(),
                clip_id: clip.clip_id.clone(),
                track_role: track.role,
                timeline_start_ms: clip.timeline_start_ms,
                source_start_ms: clip.source_start_ms,
                duration_ms: clip.duration_ms,
                label: clip.label.clone(),
            });
        }
    }

    Ok(ResolvedInterchangeInput { assets, clips })
}

fn render_fcpxml(
    profile: FcpxmlExportProfileV1,
    production_pack: &ProductionPackV1,
    resolved: &ResolvedInterchangeInput,
) -> Result<String> {
    let mut resource_ids = BTreeMap::new();
    for (index, artifact_id) in resolved.assets.keys().enumerate() {
        resource_ids.insert(artifact_id.clone(), format!("r{}", index + 2));
    }

    let mut timeline_duration_ms = 1_u64;
    for clip in &resolved.clips {
        timeline_duration_ms = timeline_duration_ms.max(
            clip.timeline_start_ms
                .checked_add(clip.duration_ms)
                .ok_or_else(|| {
                    Error::InvalidContract(format!(
                        "timeline clip {} end time overflows u64 milliseconds",
                        clip.clip_id
                    ))
                })?,
        );
    }
    for marker in &production_pack.markers {
        timeline_duration_ms = timeline_duration_ms.max(marker.position_ms.saturating_add(1));
    }

    let frame_duration = format!(
        "{}/{}s",
        production_pack.frame_rate.denominator, production_pack.frame_rate.numerator
    );
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<fcpxml version=\"");
    xml.push_str(profile.compatibility.version());
    xml.push_str("\">\n");
    xml.push_str("  <resources>\n");
    xml.push_str("    <format id=\"r1\" name=\"OmniCreator Timeline\" frameDuration=\"");
    xml.push_str(&frame_duration);
    xml.push_str("\"/>\n");

    for (artifact_id, asset) in &resolved.assets {
        let resource_id = resource_ids.get(artifact_id).ok_or_else(|| {
            Error::InvalidContract("missing deterministic resource id".to_owned())
        })?;
        xml.push_str("    <asset id=\"");
        xml.push_str(resource_id);
        xml.push_str("\" name=\"");
        xml.push_str(&xml_escape_attribute(&asset.artifact_id));
        xml.push_str("\" start=\"0s\" duration=\"");
        xml.push_str(&format_time_ms(asset.source_duration_ms));
        xml.push('"');
        if asset.has_video {
            xml.push_str(" hasVideo=\"1\" format=\"r1\"");
        }
        if asset.has_audio {
            xml.push_str(" hasAudio=\"1\"");
        }
        xml.push_str(">\n");
        xml.push_str("      <media-rep kind=\"original-media\" src=\"");
        xml.push_str(&xml_escape_attribute(&asset.file_url));
        xml.push_str("\"/>\n");
        xml.push_str("    </asset>\n");
    }

    xml.push_str("  </resources>\n");
    xml.push_str("  <event name=\"");
    xml.push_str(&xml_escape_attribute(&production_pack.title));
    xml.push_str("\">\n");
    xml.push_str("    <project name=\"");
    xml.push_str(&xml_escape_attribute(&production_pack.title));
    xml.push_str("\">\n");
    xml.push_str("      <sequence format=\"r1\" duration=\"");
    xml.push_str(&format_time_ms(timeline_duration_ms));
    xml.push_str("\">\n");
    xml.push_str("        <spine>\n");
    xml.push_str(
        "          <gap name=\"OmniCreator Timeline\" offset=\"0s\" start=\"0s\" duration=\"",
    );
    xml.push_str(&format_time_ms(timeline_duration_ms));
    xml.push_str("\">\n");

    for clip in &resolved.clips {
        let resource_id = resource_ids
            .get(&clip.artifact_id)
            .ok_or_else(|| Error::InvalidContract("clip resource is missing".to_owned()))?;
        let clip_name = match clip.label.as_deref() {
            Some(label) => format!("{} | {}", clip.track_role.stable_name_v1(), label),
            None => format!("{} | {}", clip.track_role.stable_name_v1(), clip.clip_id),
        };
        xml.push_str("            <asset-clip name=\"");
        xml.push_str(&xml_escape_attribute(&clip_name));
        xml.push_str("\" ref=\"");
        xml.push_str(resource_id);
        xml.push_str("\" lane=\"");
        xml.push_str(&lane_for_role(clip.track_role).to_string());
        xml.push_str("\" offset=\"");
        xml.push_str(&format_time_ms(clip.timeline_start_ms));
        xml.push_str("\" start=\"");
        xml.push_str(&format_time_ms(clip.source_start_ms));
        xml.push_str("\" duration=\"");
        xml.push_str(&format_time_ms(clip.duration_ms));
        xml.push('"');
        if is_video_role(clip.track_role) {
            xml.push_str(" videoRole=\"");
            xml.push_str(video_role_name(clip.track_role));
            xml.push('"');
        } else {
            xml.push_str(" audioRole=\"");
            xml.push_str(audio_role_name(clip.track_role));
            xml.push('"');
        }
        xml.push_str("/>\n");
    }

    for marker in &production_pack.markers {
        let marker_value = format!("{} | {}", marker_kind_name(marker.kind), marker.label);
        xml.push_str("            <marker start=\"");
        xml.push_str(&format_time_ms(marker.position_ms));
        xml.push_str("\" duration=\"");
        xml.push_str(&frame_duration);
        xml.push_str("\" value=\"");
        xml.push_str(&xml_escape_attribute(&marker_value));
        xml.push_str("\"/>\n");
    }

    xml.push_str("          </gap>\n");
    xml.push_str("        </spine>\n");
    xml.push_str("      </sequence>\n");
    xml.push_str("    </project>\n");
    xml.push_str("  </event>\n");
    xml.push_str("</fcpxml>\n");
    Ok(xml)
}

fn format_time_ms(milliseconds: u64) -> String {
    if milliseconds % 1000 == 0 {
        format!("{}s", milliseconds / 1000)
    } else {
        format!("{milliseconds}/1000s")
    }
}

fn lane_for_role(role: TimelineTrackRoleV1) -> i8 {
    match role {
        TimelineTrackRoleV1::VideoBackground => 1,
        TimelineTrackRoleV1::VideoPrimary => 2,
        TimelineTrackRoleV1::VideoBroll => 3,
        TimelineTrackRoleV1::VideoGeneratedOverlay => 4,
        TimelineTrackRoleV1::VideoTypographyScripture => 5,
        TimelineTrackRoleV1::AudioNarration => -1,
        TimelineTrackRoleV1::AudioMusic => -2,
        TimelineTrackRoleV1::AudioAmbience => -3,
        TimelineTrackRoleV1::AudioSfx => -4,
    }
}

fn is_video_role(role: TimelineTrackRoleV1) -> bool {
    matches!(
        role,
        TimelineTrackRoleV1::VideoBackground
            | TimelineTrackRoleV1::VideoPrimary
            | TimelineTrackRoleV1::VideoBroll
            | TimelineTrackRoleV1::VideoGeneratedOverlay
            | TimelineTrackRoleV1::VideoTypographyScripture
    )
}

fn video_role_name(role: TimelineTrackRoleV1) -> &'static str {
    match role {
        TimelineTrackRoleV1::VideoBackground => "video.background",
        TimelineTrackRoleV1::VideoPrimary => "video.primary",
        TimelineTrackRoleV1::VideoBroll => "video.broll",
        TimelineTrackRoleV1::VideoGeneratedOverlay => "video.generated-overlays",
        TimelineTrackRoleV1::VideoTypographyScripture => "video.typography-scripture",
        _ => "video",
    }
}

fn audio_role_name(role: TimelineTrackRoleV1) -> &'static str {
    match role {
        TimelineTrackRoleV1::AudioNarration => "dialogue.narration",
        TimelineTrackRoleV1::AudioMusic => "music.music",
        TimelineTrackRoleV1::AudioAmbience => "effects.ambience",
        TimelineTrackRoleV1::AudioSfx => "effects.sfx",
        _ => "effects",
    }
}

fn marker_kind_name(kind: TimelineMarkerKindV1) -> &'static str {
    match kind {
        TimelineMarkerKindV1::Scene => "Scene",
        TimelineMarkerKindV1::Scripture => "Scripture",
        TimelineMarkerKindV1::Review => "Review",
        TimelineMarkerKindV1::Chapter => "Chapter",
        TimelineMarkerKindV1::Custom => "Custom",
    }
}

fn xml_escape_attribute(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

pub fn file_url_from_absolute_path(path: &Path) -> Result<String> {
    if !path.is_absolute() {
        return Err(Error::InvalidExportPath(path.to_path_buf()));
    }
    let raw = path
        .to_str()
        .ok_or_else(|| Error::InvalidPathEncoding(path.to_path_buf()))?;
    let normalized = raw.replace('\\', "/");

    if normalized.starts_with("//") {
        let without_prefix = normalized.trim_start_matches('/');
        return Ok(format!(
            "file://{}",
            percent_encode_url_path(without_prefix)
        ));
    }

    let encoded = percent_encode_url_path(&normalized);
    if normalized.starts_with('/') {
        Ok(format!("file://{encoded}"))
    } else {
        Ok(format!("file:///{encoded}"))
    }
}

fn percent_encode_url_path(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                encoded.push(char::from(*byte))
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn xml_attribute_escaping_is_complete_for_reserved_characters() {
        assert_eq!(
            xml_escape_attribute("A & <B> \"quote\" 'single'"),
            "A &amp; &lt;B&gt; &quot;quote&quot; &apos;single&apos;"
        );
    }

    #[test]
    fn file_url_escapes_spaces_unicode_hash_and_percent() {
        #[cfg(unix)]
        {
            let url =
                file_url_from_absolute_path(Path::new("/tmp/Omni Creator/ảnh #100%.mp4")).unwrap();
            assert_eq!(
                url,
                "file:///tmp/Omni%20Creator/%E1%BA%A3nh%20%23100%25.mp4"
            );
        }
    }

    #[test]
    fn stable_lane_mapping_matches_phase9_contract() {
        let roles = [
            TimelineTrackRoleV1::VideoBackground,
            TimelineTrackRoleV1::VideoPrimary,
            TimelineTrackRoleV1::VideoBroll,
            TimelineTrackRoleV1::VideoGeneratedOverlay,
            TimelineTrackRoleV1::VideoTypographyScripture,
            TimelineTrackRoleV1::AudioNarration,
            TimelineTrackRoleV1::AudioMusic,
            TimelineTrackRoleV1::AudioAmbience,
            TimelineTrackRoleV1::AudioSfx,
        ];
        let lanes = roles
            .into_iter()
            .map(lane_for_role)
            .collect::<BTreeSet<_>>();
        assert_eq!(lanes.len(), 9);
        assert_eq!(lane_for_role(TimelineTrackRoleV1::VideoBackground), 1);
        assert_eq!(
            lane_for_role(TimelineTrackRoleV1::VideoTypographyScripture),
            5
        );
        assert_eq!(lane_for_role(TimelineTrackRoleV1::AudioNarration), -1);
        assert_eq!(lane_for_role(TimelineTrackRoleV1::AudioSfx), -4);
    }
}
