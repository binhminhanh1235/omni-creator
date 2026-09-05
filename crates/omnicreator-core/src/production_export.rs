use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    artifact_store::{AttemptOutputPromotion, AttemptPromotionRequest},
    deterministic_input_hash, Artifact, ArtifactStore, Attempt, Error, FcpxmlExportProfileV1,
    FcpxmlExporterV1, Job, LogicalUri, ProductionPackV1, Result, StateStore,
};

pub const PRODUCTION_PACKAGE_LAYOUT_VERSION_V1: u32 = 1;
pub const PRODUCTION_PACKAGE_EXPORTER_VERSION_V1: &str = "phase9-p2-v1";
pub const ASSET_SOURCE_REPORT_SCHEMA_V1: &str = "omnicreator.asset-source-report";

const PORTABLE_PROVENANCE_KEYS_V1: &[&str] = &[
    "source",
    "source_provider",
    "source_asset_id",
    "selection_ref",
    "source_page_url",
    "creator_name",
    "creator_url",
    "provider",
    "model",
    "seed",
    "style",
    "resolution",
    "aspect_ratio",
    "prompt_sha256",
    "settings",
    "settings_fingerprint",
    "mime_type",
    "provider_metadata",
    "provenance",
    "use_case",
    "visual_routing",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProductionPackageLayoutV1 {
    pub version: u32,
    pub stable_project_folder: String,
    pub export_variant: String,
    pub srt_uri: LogicalUri,
    pub fcpxml_uri: LogicalUri,
    pub source_report_uri: LogicalUri,
    pub production_pack_uri: LogicalUri,
}

impl ProductionPackageLayoutV1 {
    pub fn for_export_v1(
        production_pack: &ProductionPackV1,
        semantic_hash: &str,
        execution_hash: &str,
    ) -> Result<Self> {
        require_sha256_v1("semantic hash", semantic_hash)?;
        require_sha256_v1("execution hash", execution_hash)?;

        let slug = sanitize_filename_v1(&production_pack.title);
        let stable_project_folder = format!("{slug}-{}", &semantic_hash[..12]);
        let export_variant = execution_hash[..12].to_owned();
        let base = format!("production/{stable_project_folder}/exports/{export_variant}");

        Ok(Self {
            version: PRODUCTION_PACKAGE_LAYOUT_VERSION_V1,
            stable_project_folder,
            export_variant,
            srt_uri: LogicalUri::parse(&format!("project://{base}/timeline/subtitles.srt"))?,
            fcpxml_uri: LogicalUri::parse(&format!("project://{base}/timeline/edit.fcpxml"))?,
            source_report_uri: LogicalUri::parse(&format!(
                "project://{base}/reports/asset-sources.json"
            ))?,
            production_pack_uri: LogicalUri::parse(&format!(
                "project://{base}/metadata/production-pack.json"
            ))?,
        })
    }

    fn expected_uris_v1(&self) -> BTreeSet<String> {
        [
            &self.srt_uri,
            &self.fcpxml_uri,
            &self.source_report_uri,
            &self.production_pack_uri,
        ]
        .into_iter()
        .map(ToString::to_string)
        .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct AssetTimelineUsageV1 {
    pub track_order: u8,
    pub track_role: String,
    pub clip_id: String,
    pub timeline_start_ms: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetSourceEntryV1 {
    pub artifact_id: String,
    pub logical_uri: LogicalUri,
    pub artifact_type: String,
    pub sha256: String,
    #[serde(default)]
    pub source_metadata: BTreeMap<String, Value>,
    #[serde(default)]
    pub timeline_usages: Vec<AssetTimelineUsageV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetSourceReportV1 {
    pub schema: String,
    pub version: u32,
    pub project_id: String,
    pub assets: Vec<AssetSourceEntryV1>,
}

#[derive(Debug, Clone)]
struct PreparedProductionExportV1 {
    normalized_pack: ProductionPackV1,
    production_pack_json: Vec<u8>,
    source_report_json: Vec<u8>,
    semantic_hash: String,
    execution_hash: String,
    layout: ProductionPackageLayoutV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProductionPackageExportOutcomeV1 {
    pub cache_hit: bool,
    pub semantic_hash: String,
    pub execution_hash: String,
    pub layout: ProductionPackageLayoutV1,
    pub job_id: Option<String>,
    pub attempt_id: Option<String>,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProductionExportHistoryEntryV1 {
    pub job: Job,
    pub attempts: Vec<Attempt>,
    pub artifacts: Vec<Artifact>,
    pub package_base_uri: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ProductionPackageExporterV1 {
    fcpxml_profile: FcpxmlExportProfileV1,
}

impl ProductionPackageExporterV1 {
    pub fn new(fcpxml_profile: FcpxmlExportProfileV1) -> Self {
        Self { fcpxml_profile }
    }

    pub fn export_v1(
        &self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        production_pack: &ProductionPackV1,
    ) -> Result<ProductionPackageExportOutcomeV1> {
        let prepared = self.prepare_v1(state_store, artifact_store, production_pack)?;

        let cached = artifact_store
            .lookup_verified_cache_artifacts(state_store, &prepared.execution_hash)?;
        if cache_matches_layout_v1(&cached, &prepared.layout) {
            return Ok(ProductionPackageExportOutcomeV1 {
                cache_hit: true,
                semantic_hash: prepared.semantic_hash,
                execution_hash: prepared.execution_hash,
                layout: prepared.layout,
                job_id: cached
                    .first()
                    .and_then(|artifact| artifact.producer_job.clone()),
                attempt_id: None,
                artifacts: cached,
            });
        }

        let job = match state_store.find_retryable_production_export_job_v1(
            &prepared.normalized_pack.project_id,
            &prepared.execution_hash,
        )? {
            Some(job) => job,
            None => state_store.create_job(
                &prepared.normalized_pack.project_id,
                "export.production-pack",
                &prepared.layout.stable_project_folder,
                &prepared.execution_hash,
            )?,
        };
        self.execute_job_v1(state_store, artifact_store, &job.job_id, prepared)
    }

    fn prepare_v1(
        &self,
        state_store: &StateStore,
        artifact_store: &ArtifactStore,
        production_pack: &ProductionPackV1,
    ) -> Result<PreparedProductionExportV1> {
        let normalized_pack = production_pack.normalized_v1()?;
        let production_pack_json = deterministic_pretty_json_v1(&normalized_pack)?;
        ensure_json_has_no_absolute_path_v1(
            &serde_json::to_value(&normalized_pack)?,
            "production pack metadata",
        )?;

        let source_report = build_asset_source_report_v1(state_store, &normalized_pack)?;
        let source_report_json = deterministic_pretty_json_v1(&source_report)?;
        ensure_json_has_no_absolute_path_v1(
            &serde_json::to_value(&source_report)?,
            "asset source report",
        )?;

        let profile_version = self.fcpxml_profile.compatibility.version();
        let layout_version = PRODUCTION_PACKAGE_LAYOUT_VERSION_V1.to_string();
        let semantic_hash = deterministic_input_hash(&[
            PRODUCTION_PACKAGE_EXPORTER_VERSION_V1.as_bytes(),
            layout_version.as_bytes(),
            profile_version.as_bytes(),
            &production_pack_json,
            &source_report_json,
        ]);

        let binding_fingerprint = deterministic_input_hash(&[
            b"production-package-current-binding-v1",
            artifact_store.data_root().to_string_lossy().as_bytes(),
        ]);
        let execution_hash = deterministic_input_hash(&[
            b"production-package-execution-v1",
            semantic_hash.as_bytes(),
            binding_fingerprint.as_bytes(),
        ]);
        let layout = ProductionPackageLayoutV1::for_export_v1(
            &normalized_pack,
            &semantic_hash,
            &execution_hash,
        )?;

        Ok(PreparedProductionExportV1 {
            normalized_pack,
            production_pack_json,
            source_report_json,
            semantic_hash,
            execution_hash,
            layout,
        })
    }

    fn execute_job_v1(
        &self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        job_id: &str,
        prepared: PreparedProductionExportV1,
    ) -> Result<ProductionPackageExportOutcomeV1> {
        let job = state_store.get_job(job_id)?;
        if job.project_id != prepared.normalized_pack.project_id
            || job.input_hash != prepared.execution_hash
        {
            return Err(Error::InvalidJobState(
                "production export job does not match prepared export input".to_owned(),
            ));
        }

        let attempt = state_store.start_attempt(job_id, Some("local-production-export"))?;
        let result = self.generate_and_promote_v1(
            state_store,
            artifact_store,
            &attempt.attempt_id,
            &job.job_id,
            &prepared,
        );

        match result {
            Ok(artifacts) => Ok(ProductionPackageExportOutcomeV1 {
                cache_hit: false,
                semantic_hash: prepared.semantic_hash,
                execution_hash: prepared.execution_hash,
                layout: prepared.layout,
                job_id: Some(job.job_id),
                attempt_id: Some(attempt.attempt_id),
                artifacts,
            }),
            Err(error) => {
                let _ =
                    state_store.finish_attempt_failure(&attempt.attempt_id, "LOCAL_EXPORT_ERROR");
                Err(error)
            }
        }
    }

    fn generate_and_promote_v1(
        &self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        attempt_id: &str,
        job_id: &str,
        prepared: &PreparedProductionExportV1,
    ) -> Result<Vec<Artifact>> {
        let staging = artifact_store
            .data_root()
            .join(".runtime")
            .join("production-export")
            .join(attempt_id);
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        fs::create_dir_all(&staging)?;

        let result = (|| {
            let srt = prepared.normalized_pack.render_srt_v1()?;
            let fcpxml = FcpxmlExporterV1::new(self.fcpxml_profile).render(
                state_store,
                artifact_store,
                &prepared.normalized_pack,
            )?;

            let srt_path = staging.join("subtitles.srt");
            let fcpxml_path = staging.join("edit.fcpxml");
            let report_path = staging.join("asset-sources.json");
            let pack_path = staging.join("production-pack.json");

            fs::write(&srt_path, srt.as_bytes())?;
            fs::write(&fcpxml_path, fcpxml.as_bytes())?;
            fs::write(&report_path, &prepared.source_report_json)?;
            fs::write(&pack_path, &prepared.production_pack_json)?;

            let common = |component: &str| {
                serde_json::json!({
                    "production_package_component": component,
                    "layout_version": PRODUCTION_PACKAGE_LAYOUT_VERSION_V1,
                    "exporter_version": PRODUCTION_PACKAGE_EXPORTER_VERSION_V1,
                    "semantic_hash": prepared.semantic_hash,
                })
            };
            artifact_store.promote_attempt_outputs(
                state_store,
                AttemptPromotionRequest {
                    attempt_id: attempt_id.to_owned(),
                    job_id: job_id.to_owned(),
                    outputs: vec![
                        AttemptOutputPromotion {
                            source: srt_path,
                            target_uri: prepared.layout.srt_uri.clone(),
                            artifact_type: "subtitle".to_owned(),
                            metadata: common("srt"),
                            expected_sha256: None,
                        },
                        AttemptOutputPromotion {
                            source: fcpxml_path,
                            target_uri: prepared.layout.fcpxml_uri.clone(),
                            artifact_type: "fcpxml".to_owned(),
                            metadata: common("fcpxml"),
                            expected_sha256: None,
                        },
                        AttemptOutputPromotion {
                            source: report_path,
                            target_uri: prepared.layout.source_report_uri.clone(),
                            artifact_type: "asset-source-report".to_owned(),
                            metadata: common("asset-source-report"),
                            expected_sha256: None,
                        },
                        AttemptOutputPromotion {
                            source: pack_path,
                            target_uri: prepared.layout.production_pack_uri.clone(),
                            artifact_type: "production-pack".to_owned(),
                            metadata: common("production-pack"),
                            expected_sha256: None,
                        },
                    ],
                    selected_output_index: 3,
                },
            )
        })();

        let _ = fs::remove_dir_all(&staging);
        result
    }
}

impl StateStore {
    pub fn production_export_history_v1(
        &self,
        project_id: &str,
    ) -> Result<Vec<ProductionExportHistoryEntryV1>> {
        self.get_project(project_id)?;
        let job_ids = {
            let mut statement = self.connection.prepare(
                "SELECT id FROM jobs \
                 WHERE project_id=?1 AND step_key='export.production-pack' \
                 ORDER BY rowid DESC",
            )?;
            let rows = statement
                .query_map([project_id], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };

        let mut history = Vec::with_capacity(job_ids.len());
        for job_id in job_ids {
            let job = self.get_job(&job_id)?;
            let attempts = self.list_attempts(&job_id)?;
            let artifact_ids = {
                let mut statement = self
                    .connection
                    .prepare("SELECT id FROM artifacts WHERE producer_job_id=?1 ORDER BY uri,id")?;
                let rows = statement
                    .query_map([job_id.as_str()], |row| row.get::<_, String>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                rows
            };
            let artifacts = artifact_ids
                .into_iter()
                .map(|artifact_id| self.get_artifact(&artifact_id))
                .collect::<Result<Vec<_>>>()?;

            history.push(ProductionExportHistoryEntryV1 {
                package_base_uri: production_package_base_uri_v1(&job)?,
                job,
                attempts,
                artifacts,
            });
        }
        Ok(history)
    }

    fn find_retryable_production_export_job_v1(
        &self,
        project_id: &str,
        input_hash: &str,
    ) -> Result<Option<Job>> {
        let job_ids = {
            let mut statement = self.connection.prepare(
                "SELECT id FROM jobs \
                 WHERE project_id=?1 AND step_key='export.production-pack' \
                   AND input_hash=?2 AND status='RETRYABLE' \
                 ORDER BY rowid DESC LIMIT 1",
            )?;
            let rows = statement
                .query_map([project_id, input_hash], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        job_ids
            .into_iter()
            .next()
            .map(|job_id| self.get_job(&job_id))
            .transpose()
    }
}

fn production_package_base_uri_v1(job: &Job) -> Result<String> {
    require_sha256_v1("production export job input hash", &job.input_hash)?;
    if job.unit.is_empty()
        || job.unit.contains('/')
        || job.unit.contains('\\')
        || job.unit.contains("..")
    {
        return Err(Error::InvalidJobState(
            "production export job unit is not a stable package folder".to_owned(),
        ));
    }
    Ok(format!(
        "project://production/{}/exports/{}/",
        job.unit,
        &job.input_hash[..12]
    ))
}

impl Default for ProductionPackageExporterV1 {
    fn default() -> Self {
        Self::new(FcpxmlExportProfileV1::default())
    }
}

impl ArtifactStore {
    pub fn lookup_verified_cache_artifacts(
        &self,
        state_store: &StateStore,
        input_hash: &str,
    ) -> Result<Vec<Artifact>> {
        let artifacts = state_store.find_cached_artifacts(input_hash)?;
        for artifact in &artifacts {
            if !self.verify_artifact(artifact)? {
                return Ok(Vec::new());
            }
        }
        Ok(artifacts)
    }
}

pub fn build_asset_source_report_v1(
    state_store: &StateStore,
    production_pack: &ProductionPackV1,
) -> Result<AssetSourceReportV1> {
    let normalized = production_pack.normalized_v1()?;
    let mut usages = BTreeMap::<String, Vec<AssetTimelineUsageV1>>::new();

    for track in &normalized.tracks {
        for clip in &track.clips {
            usages
                .entry(clip.artifact_id.clone())
                .or_default()
                .push(AssetTimelineUsageV1 {
                    track_order: track.role.stable_order_v1(),
                    track_role: track.role.stable_name_v1().to_owned(),
                    clip_id: clip.clip_id.clone(),
                    timeline_start_ms: clip.timeline_start_ms,
                    duration_ms: clip.duration_ms,
                });
        }
    }

    let mut assets = Vec::with_capacity(usages.len());
    for (artifact_id, mut timeline_usages) in usages {
        let artifact = state_store.get_artifact(&artifact_id)?;
        if artifact.project_id.as_deref() != Some(normalized.project_id.as_str()) {
            return Err(Error::ExportArtifactProjectMismatch {
                artifact_id: artifact.artifact_id,
                expected_project: normalized.project_id.clone(),
                actual_project: artifact.project_id,
            });
        }

        timeline_usages.sort();
        let source_metadata = portable_source_metadata_v1(&artifact.metadata)?;
        assets.push(AssetSourceEntryV1 {
            artifact_id: artifact.artifact_id,
            logical_uri: artifact.uri,
            artifact_type: artifact.artifact_type,
            sha256: artifact.sha256,
            source_metadata,
            timeline_usages,
        });
    }
    assets.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));

    Ok(AssetSourceReportV1 {
        schema: ASSET_SOURCE_REPORT_SCHEMA_V1.to_owned(),
        version: 1,
        project_id: normalized.project_id,
        assets,
    })
}

fn portable_source_metadata_v1(metadata: &Value) -> Result<BTreeMap<String, Value>> {
    let Some(object) = metadata.as_object() else {
        return Ok(BTreeMap::new());
    };
    let mut portable = BTreeMap::new();
    for key in PORTABLE_PROVENANCE_KEYS_V1 {
        if let Some(value) = object.get(*key) {
            ensure_json_has_no_absolute_path_v1(value, key)?;
            portable.insert((*key).to_owned(), canonicalize_json_v1(value));
        }
    }
    Ok(portable)
}

fn canonicalize_json_v1(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonicalize_json_v1).collect()),
        Value::Object(object) => {
            let ordered = object
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize_json_v1(value)))
                .collect::<BTreeMap<_, _>>();
            serde_json::to_value(ordered).expect("BTreeMap JSON serialization is infallible")
        }
        _ => value.clone(),
    }
}

fn ensure_json_has_no_absolute_path_v1(value: &Value, label: &str) -> Result<()> {
    match value {
        Value::String(value) if looks_like_absolute_path_v1(value) => Err(Error::InvalidContract(
            format!("{label} contains a machine-specific absolute path"),
        )),
        Value::Array(values) => {
            for value in values {
                ensure_json_has_no_absolute_path_v1(value, label)?;
            }
            Ok(())
        }
        Value::Object(object) => {
            for value in object.values() {
                ensure_json_has_no_absolute_path_v1(value, label)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn looks_like_absolute_path_v1(value: &str) -> bool {
    if value.starts_with('/') || value.starts_with("\\\\") {
        return true;
    }
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

fn deterministic_pretty_json_v1<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn cache_matches_layout_v1(artifacts: &[Artifact], layout: &ProductionPackageLayoutV1) -> bool {
    if artifacts.len() != 4 {
        return false;
    }
    artifacts
        .iter()
        .map(|artifact| artifact.uri.to_string())
        .collect::<BTreeSet<_>>()
        == layout.expected_uris_v1()
}

fn require_sha256_v1(label: &str, value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::InvalidContract(format!(
            "{label} must be a 64-character hexadecimal SHA256"
        )));
    }
    Ok(())
}

pub fn sanitize_filename_v1(value: &str) -> String {
    let mut output = String::new();
    let mut separator_pending = false;

    for character in value.trim().chars() {
        if character.is_alphanumeric() {
            if separator_pending && !output.is_empty() {
                output.push('-');
            }
            separator_pending = false;
            for lowercase in character.to_lowercase() {
                if output.chars().count() >= 64 {
                    break;
                }
                output.push(lowercase);
            }
        } else if !output.is_empty() {
            separator_pending = true;
        }
        if output.chars().count() >= 64 {
            break;
        }
    }

    while output.ends_with('-') {
        output.pop();
    }
    if output.is_empty() {
        "project".to_owned()
    } else {
        output
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{
        ProductionPackV1, StepStatus, SubtitleCueV1, TimelineClipV1, TimelineFrameRateV1,
        TimelineTrackRoleV1, TimelineTrackV1, Workspace, PRODUCTION_PACK_SCHEMA_V1,
        PRODUCTION_PACK_VERSION_V1,
    };

    fn promote_source_artifact(
        state: &mut StateStore,
        store: &ArtifactStore,
        temp: &Path,
        project_id: &str,
        bytes: &[u8],
        metadata: Value,
    ) -> Artifact {
        let input_hash = deterministic_input_hash(&[b"source", bytes]);
        let job = state
            .create_job(project_id, "visual", "SC01", &input_hash)
            .unwrap();
        let source = temp.join(format!("source-{}.mp4", &input_hash[..8]));
        fs::write(&source, bytes).unwrap();
        store
            .promote_job_output(
                state,
                &job.job_id,
                source,
                LogicalUri::parse("project://video/SC01.mp4").unwrap(),
                "video",
                metadata,
            )
            .unwrap()
    }

    fn pack(project_id: &str, artifact: &Artifact, title: &str) -> ProductionPackV1 {
        ProductionPackV1 {
            schema: PRODUCTION_PACK_SCHEMA_V1.to_owned(),
            version: PRODUCTION_PACK_VERSION_V1,
            project_id: project_id.to_owned(),
            title: title.to_owned(),
            frame_rate: TimelineFrameRateV1 {
                numerator: 24,
                denominator: 1,
            },
            tracks: vec![TimelineTrackV1 {
                role: TimelineTrackRoleV1::VideoPrimary,
                clips: vec![TimelineClipV1 {
                    clip_id: "SC01-V".to_owned(),
                    artifact_id: artifact.artifact_id.clone(),
                    uri: artifact.uri.clone(),
                    timeline_start_ms: 0,
                    source_start_ms: 0,
                    duration_ms: 2_000,
                    label: Some("Opening".to_owned()),
                }],
            }],
            subtitles: vec![SubtitleCueV1 {
                cue_id: "S01".to_owned(),
                start_ms: 0,
                end_ms: 2_000,
                text: "Hello world".to_owned(),
            }],
            markers: Vec::new(),
        }
    }

    #[test]
    fn filename_sanitization_is_unicode_safe_and_deterministic() {
        assert_eq!(
            sanitize_filename_v1("  Café / Faith: Episode 01 ✨  "),
            "café-faith-episode-01"
        );
        assert_eq!(sanitize_filename_v1("../../"), "project");
        assert_eq!(
            sanitize_filename_v1("Café / Faith: Episode 01 ✨"),
            sanitize_filename_v1("Café / Faith: Episode 01 ✨")
        );
    }

    #[test]
    fn source_report_uses_canonical_metadata_and_stable_usage_order() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = state.create_project("Report").unwrap();
        let store = ArtifactStore::new(workspace.data_root()).unwrap();
        let artifact = promote_source_artifact(
            &mut state,
            &store,
            temp.path(),
            &project.id,
            b"asset",
            serde_json::json!({
                "source_provider": "pexels",
                "source_asset_id": "2499611",
                "selection_ref": "pexels:video:2499611",
                "provenance": {
                    "creator_name": "Creator",
                    "source_page_url": "https://www.pexels.com/video/2499611/"
                }
            }),
        );
        let mut production_pack = pack(&project.id, &artifact, "Report");
        let first_clip = production_pack.tracks[0].clips[0].clone();
        production_pack.tracks[0].clips = vec![
            TimelineClipV1 {
                clip_id: "SC02-V".to_owned(),
                artifact_id: artifact.artifact_id.clone(),
                uri: artifact.uri.clone(),
                timeline_start_ms: 2_000,
                source_start_ms: 0,
                duration_ms: 1_000,
                label: Some("Second".to_owned()),
            },
            first_clip,
        ];
        let report = build_asset_source_report_v1(&state, &production_pack).unwrap();
        let report_again = build_asset_source_report_v1(&state, &production_pack).unwrap();

        assert_eq!(report, report_again);
        assert_eq!(report.assets.len(), 1);
        assert_eq!(
            report.assets[0].source_metadata["source_provider"],
            "pexels"
        );
        assert_eq!(
            report.assets[0].source_metadata["provenance"]["creator_name"],
            "Creator"
        );
        assert_eq!(
            report.assets[0]
                .timeline_usages
                .iter()
                .map(|usage| usage.clip_id.as_str())
                .collect::<Vec<_>>(),
            vec!["SC01-V", "SC02-V"]
        );
    }

    #[test]
    fn source_report_tolerates_missing_optional_provenance_and_rejects_absolute_path_leak() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = state.create_project("Portable report").unwrap();
        let store = ArtifactStore::new(workspace.data_root()).unwrap();
        let artifact = promote_source_artifact(
            &mut state,
            &store,
            temp.path(),
            &project.id,
            b"asset",
            serde_json::json!({"source_provider": "fixture"}),
        );
        let report =
            build_asset_source_report_v1(&state, &pack(&project.id, &artifact, "Portable"))
                .unwrap();
        assert_eq!(report.assets[0].source_metadata.len(), 1);

        assert!(portable_source_metadata_v1(&serde_json::json!({
            "provenance": {"local_path": "/Users/alice/media.mov"}
        }))
        .is_err());
    }

    #[test]
    fn export_packages_all_components_records_attempt_and_hits_verified_cache() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = state.create_project("Café Production").unwrap();
        let store = ArtifactStore::new(workspace.data_root()).unwrap();
        let artifact = promote_source_artifact(
            &mut state,
            &store,
            temp.path(),
            &project.id,
            b"asset",
            serde_json::json!({"source_provider": "fixture"}),
        );
        let production_pack = pack(&project.id, &artifact, "Café Production");
        let exporter = ProductionPackageExporterV1::default();

        let first = exporter
            .export_v1(&mut state, &store, &production_pack)
            .unwrap();
        assert!(!first.cache_hit);
        assert_eq!(first.artifacts.len(), 4);
        let job = state.get_job(first.job_id.as_deref().unwrap()).unwrap();
        assert_eq!(job.status, StepStatus::Succeeded);
        let attempt = state
            .get_attempt(first.attempt_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(attempt.status, StepStatus::Succeeded);

        for artifact in &first.artifacts {
            assert!(store.verify_artifact(artifact).unwrap());
        }
        let srt = first
            .artifacts
            .iter()
            .find(|artifact| artifact.artifact_type == "subtitle")
            .unwrap();
        assert!(
            fs::read_to_string(store.resolve_artifact_path(srt).unwrap())
                .unwrap()
                .contains("00:00:00,000 --> 00:00:02,000")
        );

        let second = exporter
            .export_v1(&mut state, &store, &production_pack)
            .unwrap();
        assert!(second.cache_hit);
        assert_eq!(first.execution_hash, second.execution_hash);
        assert_eq!(first.layout, second.layout);
    }

    #[test]
    fn semantic_hash_changes_with_pack_or_relevant_provenance() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = state.create_project("Hash").unwrap();
        let store = ArtifactStore::new(workspace.data_root()).unwrap();
        let artifact = promote_source_artifact(
            &mut state,
            &store,
            temp.path(),
            &project.id,
            b"asset",
            serde_json::json!({"source_provider": "fixture", "source_asset_id": "1"}),
        );
        let exporter = ProductionPackageExporterV1::default();
        let original = exporter
            .prepare_v1(&state, &store, &pack(&project.id, &artifact, "Hash"))
            .unwrap();

        let mut changed_pack = pack(&project.id, &artifact, "Hash changed");
        changed_pack.subtitles[0].text = "Changed".to_owned();
        let changed = exporter.prepare_v1(&state, &store, &changed_pack).unwrap();
        assert_ne!(original.semantic_hash, changed.semantic_hash);

        state
            .connection
            .execute(
                "UPDATE artifacts SET metadata_json=?1 WHERE id=?2",
                rusqlite::params![
                    serde_json::json!({
                        "source_provider": "fixture",
                        "source_asset_id": "2"
                    })
                    .to_string(),
                    &artifact.artifact_id
                ],
            )
            .unwrap();
        let metadata_changed = exporter
            .prepare_v1(&state, &store, &pack(&project.id, &artifact, "Hash"))
            .unwrap();
        assert_ne!(original.semantic_hash, metadata_changed.semantic_hash);

        state
            .connection
            .execute(
                "UPDATE artifacts SET metadata_json=?1,sha256=?2 WHERE id=?3",
                rusqlite::params![
                    serde_json::json!({
                        "source_provider": "fixture",
                        "source_asset_id": "1"
                    })
                    .to_string(),
                    "0".repeat(64),
                    &artifact.artifact_id
                ],
            )
            .unwrap();
        let source_hash_changed = exporter
            .prepare_v1(&state, &store, &pack(&project.id, &artifact, "Hash"))
            .unwrap();
        assert_ne!(original.semantic_hash, source_hash_changed.semantic_hash);
    }

    #[test]
    fn failed_export_marks_attempt_and_job_retryable_without_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let mut state = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = state.create_project("Retryable export").unwrap();
        let store = ArtifactStore::new(workspace.data_root()).unwrap();
        let artifact = promote_source_artifact(
            &mut state,
            &store,
            temp.path(),
            &project.id,
            b"asset",
            serde_json::json!({"source_provider": "fixture"}),
        );
        let production_pack = pack(&project.id, &artifact, "Retryable export");
        fs::remove_file(store.resolve_artifact_path(&artifact).unwrap()).unwrap();

        let result =
            ProductionPackageExporterV1::default().export_v1(&mut state, &store, &production_pack);
        assert!(result.is_err());

        let (job_id, job_status, selected_artifact): (String, String, Option<String>) = state
            .connection
            .query_row(
                "SELECT id,status,selected_artifact_id FROM jobs \
                 WHERE project_id=?1 AND step_key='export.production-pack' \
                 ORDER BY rowid DESC LIMIT 1",
                [&project.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(job_status, "RETRYABLE");
        assert!(selected_artifact.is_none());

        let (attempt_status, error_code): (String, Option<String>) = state
            .connection
            .query_row(
                "SELECT status,error_code FROM attempts WHERE job_id=?1 \
                 ORDER BY rowid DESC LIMIT 1",
                [&job_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(attempt_status, "RETRYABLE");
        assert_eq!(error_code.as_deref(), Some("LOCAL_EXPORT_ERROR"));

        let artifact_count: i64 = state
            .connection
            .query_row(
                "SELECT COUNT(*) FROM artifacts WHERE producer_job_id=?1",
                [&job_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(artifact_count, 0);

        let source_path = store.resolve_artifact_path(&artifact).unwrap();
        fs::write(&source_path, b"asset").unwrap();
        let recovered = ProductionPackageExporterV1::default()
            .export_v1(&mut state, &store, &production_pack)
            .unwrap();

        assert_eq!(recovered.job_id.as_deref(), Some(job_id.as_str()));
        let attempts = state.list_attempts(&job_id).unwrap();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].status, StepStatus::Retryable);
        assert_eq!(
            attempts[0].error_code.as_deref(),
            Some("LOCAL_EXPORT_ERROR")
        );
        assert_eq!(attempts[1].status, StepStatus::Succeeded);

        let history = state.production_export_history_v1(&project.id).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].job.job_id, job_id);
        assert_eq!(history[0].attempts.len(), 2);
        assert_eq!(history[0].artifacts.len(), 4);
        assert!(history[0]
            .package_base_uri
            .starts_with("project://production/"));

        drop(state);
        let reopened = StateStore::open(workspace.sqlite_path()).unwrap();
        let after_restart = reopened.production_export_history_v1(&project.id).unwrap();
        assert_eq!(after_restart, history);
    }

    #[test]
    fn data_root_rebind_regenerates_fcpxml_without_rewriting_portable_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let root_a = temp.path().join("root-a");
        let workspace_a = Workspace::create(&root_a).unwrap();
        let mut state_a = StateStore::open(workspace_a.sqlite_path()).unwrap();
        let project = state_a.create_project("Portable").unwrap();
        let store_a = ArtifactStore::new(workspace_a.data_root()).unwrap();
        let source = promote_source_artifact(
            &mut state_a,
            &store_a,
            temp.path(),
            &project.id,
            b"asset",
            serde_json::json!({"source_provider": "fixture"}),
        );
        let production_pack = pack(&project.id, &source, "Portable");
        let exporter = ProductionPackageExporterV1::default();
        let exported_a = exporter
            .export_v1(&mut state_a, &store_a, &production_pack)
            .unwrap();
        let pack_artifact_a = exported_a
            .artifacts
            .iter()
            .find(|artifact| artifact.artifact_type == "production-pack")
            .unwrap();
        let portable_a = fs::read(store_a.resolve_artifact_path(pack_artifact_a).unwrap()).unwrap();
        drop(state_a);

        let root_b = temp.path().join("root-b");
        copy_dir_all_v1(&root_a, &root_b).unwrap();
        let workspace_b = Workspace::open(&root_b).unwrap();
        let mut state_b = StateStore::open(workspace_b.sqlite_path()).unwrap();
        let store_b = ArtifactStore::new(workspace_b.data_root()).unwrap();
        let exported_b = exporter
            .export_v1(&mut state_b, &store_b, &production_pack)
            .unwrap();

        assert!(!exported_b.cache_hit);
        assert_eq!(exported_a.semantic_hash, exported_b.semantic_hash);
        assert_ne!(exported_a.execution_hash, exported_b.execution_hash);
        let fcpxml_b = exported_b
            .artifacts
            .iter()
            .find(|artifact| artifact.artifact_type == "fcpxml")
            .unwrap();
        let xml_b = fs::read_to_string(store_b.resolve_artifact_path(fcpxml_b).unwrap()).unwrap();
        assert!(xml_b.contains("root-b"));
        assert!(!xml_b.contains("root-a"));

        let pack_artifact_b = exported_b
            .artifacts
            .iter()
            .find(|artifact| artifact.artifact_type == "production-pack")
            .unwrap();
        let portable_b = fs::read(store_b.resolve_artifact_path(pack_artifact_b).unwrap()).unwrap();
        assert_eq!(portable_a, portable_b);
    }

    fn copy_dir_all_v1(source: &Path, destination: &Path) -> std::io::Result<()> {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let target = destination.join(entry.file_name());
            if file_type.is_dir() {
                copy_dir_all_v1(&entry.path(), &target)?;
            } else {
                fs::copy(entry.path(), target)?;
            }
        }
        Ok(())
    }
}
