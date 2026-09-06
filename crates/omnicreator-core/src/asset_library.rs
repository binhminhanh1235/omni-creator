use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    state::artifact_from_row, Artifact, AssetV1, Error, Result, StateStore, ASSET_SCHEMA,
    ASSET_SCHEMA_VERSION,
};

pub const ASSET_LIBRARY_RECENT_DAYS_V1: i64 = 30;
const MEDIA_ARTIFACT_TYPES_V1: &[&str] = &["audio", "image", "thumbnail", "video"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetUsageV1 {
    pub artifact_id: String,
    pub project_id: Option<String>,
    pub usage_kind: String,
    pub usage_key: String,
    pub used_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct AssetSourceIdentityV1 {
    pub provider: String,
    pub source_asset_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssetLibraryEntryV1 {
    pub asset: AssetV1,
    pub project_id: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub usage_history: Vec<AssetUsageV1>,
    pub usage_count: u32,
    pub last_used_at: Option<DateTime<Utc>>,
    pub used_recently: bool,
    #[serde(default)]
    pub duplicate_asset_ids: Vec<String>,
    pub source_identity: Option<AssetSourceIdentityV1>,
    #[serde(default)]
    pub source_reuse_asset_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssetLibrarySnapshotV1 {
    pub generated_at: DateTime<Utc>,
    pub recent_days: i64,
    pub total_assets: usize,
    pub duplicate_groups: usize,
    pub source_reuse_groups: usize,
    pub entries: Vec<AssetLibraryEntryV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetReuseFactsV1 {
    pub artifact_ids: Vec<String>,
    pub usage_count: u32,
    pub last_used_at: Option<DateTime<Utc>>,
    pub used_recently: bool,
}

impl StateStore {
    pub fn add_asset_tag_v1(&self, artifact_id: &str, tag: &str) -> Result<String> {
        require_library_asset_v1(self, artifact_id)?;
        let normalized = normalize_asset_tag_v1(tag)?;
        self.connection.execute(
            "INSERT OR IGNORE INTO artifact_tags(artifact_id,tag,created_at) VALUES (?1,?2,?3)",
            params![artifact_id, &normalized, Utc::now().to_rfc3339()],
        )?;
        Ok(normalized)
    }

    pub fn remove_asset_tag_v1(&self, artifact_id: &str, tag: &str) -> Result<()> {
        require_library_asset_v1(self, artifact_id)?;
        let normalized = normalize_asset_tag_v1(tag)?;
        self.connection.execute(
            "DELETE FROM artifact_tags WHERE artifact_id=?1 AND tag=?2",
            params![artifact_id, normalized],
        )?;
        Ok(())
    }

    pub fn replace_asset_tags_v1(
        &mut self,
        artifact_id: &str,
        tags: &[String],
    ) -> Result<Vec<String>> {
        require_library_asset_v1(self, artifact_id)?;
        let normalized = tags
            .iter()
            .map(|tag| normalize_asset_tag_v1(tag))
            .collect::<Result<BTreeSet<_>>>()?;
        let normalized = normalized.into_iter().collect::<Vec<_>>();

        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM artifact_tags WHERE artifact_id=?1",
            [artifact_id],
        )?;
        let created_at = Utc::now().to_rfc3339();
        for tag in &normalized {
            transaction.execute(
                "INSERT INTO artifact_tags(artifact_id,tag,created_at) VALUES (?1,?2,?3)",
                params![artifact_id, tag, &created_at],
            )?;
        }
        transaction.commit()?;
        Ok(normalized)
    }

    pub fn list_asset_tags_v1(&self, artifact_id: &str) -> Result<Vec<String>> {
        require_library_asset_v1(self, artifact_id)?;
        let mut statement = self.connection.prepare(
            "SELECT tag FROM artifact_tags WHERE artifact_id=?1 ORDER BY tag",
        )?;
        Ok(statement
            .query_map([artifact_id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn record_asset_usage_v1(
        &self,
        artifact_id: &str,
        project_id: Option<&str>,
        usage_kind: &str,
        usage_key: &str,
        used_at: DateTime<Utc>,
    ) -> Result<AssetUsageV1> {
        require_library_asset_v1(self, artifact_id)?;
        if let Some(project_id) = project_id {
            self.get_project(project_id)?;
        }
        let usage_kind = normalize_usage_kind_v1(usage_kind)?;
        let usage_key = normalize_usage_key_v1(usage_key)?;
        record_asset_usage_on_connection_v1(
            &self.connection,
            artifact_id,
            project_id,
            &usage_kind,
            &usage_key,
            used_at,
        )?;
        Ok(AssetUsageV1 {
            artifact_id: artifact_id.to_owned(),
            project_id: project_id.map(str::to_owned),
            usage_kind,
            usage_key,
            used_at,
        })
    }

    pub fn list_asset_usage_v1(&self, artifact_id: &str) -> Result<Vec<AssetUsageV1>> {
        require_library_asset_v1(self, artifact_id)?;
        list_usage_v1(&self.connection, artifact_id)
    }

    pub fn asset_library_snapshot_v1(
        &self,
        reference_time: DateTime<Utc>,
    ) -> Result<AssetLibrarySnapshotV1> {
        let artifacts = list_library_artifacts_v1(&self.connection)?;
        let recent_cutoff = reference_time - Duration::days(ASSET_LIBRARY_RECENT_DAYS_V1);

        let mut tags_by_artifact = BTreeMap::<String, Vec<String>>::new();
        {
            let mut statement = self
                .connection
                .prepare("SELECT artifact_id,tag FROM artifact_tags ORDER BY artifact_id,tag")?;
            for row in statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })? {
                let (artifact_id, tag) = row?;
                tags_by_artifact.entry(artifact_id).or_default().push(tag);
            }
        }

        let mut usages_by_artifact = BTreeMap::<String, Vec<AssetUsageV1>>::new();
        {
            let mut statement = self.connection.prepare(
                "SELECT artifact_id,project_id,usage_kind,usage_key,used_at                  FROM artifact_usages ORDER BY artifact_id,used_at DESC,usage_kind,usage_key",
            )?;
            for row in statement.query_map([], usage_from_row)? {
                let usage = row?;
                usages_by_artifact
                    .entry(usage.artifact_id.clone())
                    .or_default()
                    .push(usage);
            }
        }

        let mut sha_groups = BTreeMap::<String, Vec<String>>::new();
        let mut source_groups = BTreeMap::<AssetSourceIdentityV1, Vec<String>>::new();
        for artifact in &artifacts {
            sha_groups
                .entry(artifact.sha256.clone())
                .or_default()
                .push(artifact.artifact_id.clone());
            if let Some(identity) = source_identity_v1(artifact) {
                source_groups
                    .entry(identity)
                    .or_default()
                    .push(artifact.artifact_id.clone());
            }
        }
        for values in sha_groups.values_mut() {
            values.sort();
        }
        for values in source_groups.values_mut() {
            values.sort();
        }

        let duplicate_groups = sha_groups.values().filter(|values| values.len() > 1).count();
        let source_reuse_groups = source_groups
            .values()
            .filter(|values| values.len() > 1)
            .count();

        let mut entries = Vec::with_capacity(artifacts.len());
        for artifact in artifacts {
            let source_identity = source_identity_v1(&artifact);
            let usage_history = usages_by_artifact
                .remove(&artifact.artifact_id)
                .unwrap_or_default();
            let last_used_at = usage_history.first().map(|usage| usage.used_at);
            let usage_count = u32::try_from(usage_history.len()).unwrap_or(u32::MAX);
            let duplicate_asset_ids = sha_groups
                .get(&artifact.sha256)
                .into_iter()
                .flatten()
                .filter(|artifact_id| *artifact_id != &artifact.artifact_id)
                .cloned()
                .collect::<Vec<_>>();
            let source_reuse_asset_ids = source_identity
                .as_ref()
                .and_then(|identity| source_groups.get(identity))
                .into_iter()
                .flatten()
                .filter(|artifact_id| *artifact_id != &artifact.artifact_id)
                .cloned()
                .collect::<Vec<_>>();

            entries.push(AssetLibraryEntryV1 {
                asset: asset_from_artifact_v1(&artifact)?,
                project_id: artifact.project_id.clone(),
                created_at: artifact.created_at,
                tags: tags_by_artifact
                    .remove(&artifact.artifact_id)
                    .unwrap_or_default(),
                usage_history,
                usage_count,
                last_used_at,
                used_recently: last_used_at.is_some_and(|value| value >= recent_cutoff),
                duplicate_asset_ids,
                source_identity,
                source_reuse_asset_ids,
            });
        }

        entries.sort_by(|left, right| {
            right
                .last_used_at
                .cmp(&left.last_used_at)
                .then_with(|| right.created_at.cmp(&left.created_at))
                .then_with(|| left.asset.asset_id.cmp(&right.asset.asset_id))
        });

        Ok(AssetLibrarySnapshotV1 {
            generated_at: reference_time,
            recent_days: ASSET_LIBRARY_RECENT_DAYS_V1,
            total_assets: entries.len(),
            duplicate_groups,
            source_reuse_groups,
            entries,
        })
    }

    pub fn source_reuse_facts_v1(
        &self,
        provider: &str,
        source_asset_id: &str,
        reference_time: DateTime<Utc>,
    ) -> Result<AssetReuseFactsV1> {
        let identity = AssetSourceIdentityV1 {
            provider: normalize_identity_part_v1("source provider", provider)?,
            source_asset_id: normalize_identity_part_v1("source asset id", source_asset_id)?,
        };
        let snapshot = self.asset_library_snapshot_v1(reference_time)?;
        let mut artifact_ids = BTreeSet::new();
        let mut usage_count = 0_u32;
        let mut last_used_at = None;

        for entry in snapshot.entries {
            if entry.source_identity.as_ref() != Some(&identity) {
                continue;
            }
            artifact_ids.insert(entry.asset.asset_id);
            usage_count = usage_count.saturating_add(entry.usage_count);
            last_used_at = match (last_used_at, entry.last_used_at) {
                (Some(current), Some(candidate)) => Some(current.max(candidate)),
                (None, candidate) => candidate,
                (current, None) => current,
            };
        }

        let recent_cutoff = reference_time - Duration::days(ASSET_LIBRARY_RECENT_DAYS_V1);
        Ok(AssetReuseFactsV1 {
            artifact_ids: artifact_ids.into_iter().collect(),
            usage_count,
            last_used_at,
            used_recently: last_used_at.is_some_and(|value| value >= recent_cutoff),
        })
    }
}

pub fn normalize_asset_tag_v1(value: &str) -> Result<String> {
    let normalized = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if normalized.is_empty() {
        return Err(Error::InvalidContract(
            "asset library tag must not be empty".to_owned(),
        ));
    }
    if normalized.chars().count() > 64 || normalized.chars().any(char::is_control) {
        return Err(Error::InvalidContract(
            "asset library tag must be at most 64 printable characters".to_owned(),
        ));
    }
    if looks_like_absolute_path_v1(&normalized) {
        return Err(Error::InvalidContract(
            "asset library tag must not contain a machine-specific absolute path".to_owned(),
        ));
    }
    Ok(normalized)
}

pub(crate) fn record_selected_artifact_usage_v1(
    connection: &Connection,
    artifact: &Artifact,
    project_id: &str,
    usage_kind: &str,
    usage_key: &str,
) -> Result<()> {
    if !is_library_artifact_type_v1(&artifact.artifact_type) {
        return Ok(());
    }
    record_asset_usage_on_connection_v1(
        connection,
        &artifact.artifact_id,
        Some(project_id),
        usage_kind,
        usage_key,
        artifact.created_at,
    )
}

fn record_asset_usage_on_connection_v1(
    connection: &Connection,
    artifact_id: &str,
    project_id: Option<&str>,
    usage_kind: &str,
    usage_key: &str,
    used_at: DateTime<Utc>,
) -> Result<()> {
    connection.execute(
        "INSERT INTO artifact_usages(artifact_id,project_id,usage_kind,usage_key,used_at)          VALUES (?1,?2,?3,?4,?5)          ON CONFLICT(artifact_id,usage_kind,usage_key) DO UPDATE SET          project_id=excluded.project_id,used_at=MAX(artifact_usages.used_at,excluded.used_at)",
        params![
            artifact_id,
            project_id,
            usage_kind,
            usage_key,
            used_at.to_rfc3339()
        ],
    )?;
    Ok(())
}

fn list_library_artifacts_v1(connection: &Connection) -> Result<Vec<Artifact>> {
    let mut statement = connection.prepare(
        "SELECT id,project_id,artifact_type,uri,sha256,size_bytes,producer_job_id,created_at,metadata_json,input_hash          FROM artifacts ORDER BY created_at DESC,id",
    )?;
    let artifacts = statement
        .query_map([], artifact_from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(artifacts
        .into_iter()
        .filter(|artifact| is_library_artifact_type_v1(&artifact.artifact_type))
        .collect())
}

fn require_library_asset_v1(state: &StateStore, artifact_id: &str) -> Result<Artifact> {
    let artifact = state.get_artifact(artifact_id)?;
    if !is_library_artifact_type_v1(&artifact.artifact_type) {
        return Err(Error::InvalidContract(format!(
            "artifact {artifact_id} type '{}' is not an Asset Library media type",
            artifact.artifact_type
        )));
    }
    Ok(artifact)
}

fn list_usage_v1(connection: &Connection, artifact_id: &str) -> Result<Vec<AssetUsageV1>> {
    let mut statement = connection.prepare(
        "SELECT artifact_id,project_id,usage_kind,usage_key,used_at          FROM artifact_usages WHERE artifact_id=?1          ORDER BY used_at DESC,usage_kind,usage_key",
    )?;
    Ok(statement
        .query_map([artifact_id], usage_from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

fn usage_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AssetUsageV1> {
    let raw: String = row.get(4)?;
    let used_at = DateTime::parse_from_rfc3339(&raw)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    Ok(AssetUsageV1 {
        artifact_id: row.get(0)?,
        project_id: row.get(1)?,
        usage_kind: row.get(2)?,
        usage_key: row.get(3)?,
        used_at,
    })
}

fn asset_from_artifact_v1(artifact: &Artifact) -> Result<AssetV1> {
    let metadata = artifact.metadata.as_object();
    let source_provider = metadata
        .and_then(|value| value.get("source_provider"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            metadata
                .and_then(|value| value.get("provenance"))
                .and_then(Value::as_object)
                .and_then(|value| value.get("provider"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    let width = metadata
        .and_then(|value| value.get("width"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    let height = metadata
        .and_then(|value| value.get("height"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    let duration = metadata
        .and_then(|value| value.get("duration"))
        .and_then(Value::as_f64);
    let provenance = metadata
        .and_then(|value| value.get("provenance"))
        .and_then(Value::as_object)
        .map(|value| {
            value
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    let asset = AssetV1 {
        schema: ASSET_SCHEMA.to_owned(),
        schema_version: ASSET_SCHEMA_VERSION,
        asset_id: artifact.artifact_id.clone(),
        asset_type: artifact.artifact_type.to_uppercase(),
        uri: artifact.uri.clone(),
        source_provider,
        width,
        height,
        duration,
        sha256: artifact.sha256.clone(),
        provenance,
    };
    asset.validate_v1()?;
    Ok(asset)
}

fn source_identity_v1(artifact: &Artifact) -> Option<AssetSourceIdentityV1> {
    let metadata = artifact.metadata.as_object()?;
    let provider = metadata
        .get("source_provider")
        .and_then(Value::as_str)
        .or_else(|| {
            metadata
                .get("provenance")
                .and_then(Value::as_object)
                .and_then(|value| value.get("provider"))
                .and_then(Value::as_str)
        })?;
    let source_asset_id = metadata
        .get("source_asset_id")
        .and_then(Value::as_str)
        .or_else(|| {
            metadata
                .get("provenance")
                .and_then(Value::as_object)
                .and_then(|value| value.get("source_id"))
                .and_then(Value::as_str)
        })?;
    let provider = normalize_identity_part_v1("source provider", provider).ok()?;
    let source_asset_id = normalize_identity_part_v1("source asset id", source_asset_id).ok()?;
    Some(AssetSourceIdentityV1 {
        provider,
        source_asset_id,
    })
}

fn is_library_artifact_type_v1(value: &str) -> bool {
    let normalized = value.trim().to_lowercase();
    MEDIA_ARTIFACT_TYPES_V1.contains(&normalized.as_str())
}

fn normalize_usage_kind_v1(value: &str) -> Result<String> {
    let normalized = value.trim().to_lowercase();
    if normalized.is_empty()
        || normalized.len() > 64
        || !normalized
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        return Err(Error::InvalidContract(
            "asset usage kind must be 1..=64 lowercase-compatible identifier characters"
                .to_owned(),
        ));
    }
    Ok(normalized)
}

fn normalize_usage_key_v1(value: &str) -> Result<String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty()
        || normalized.chars().count() > 256
        || normalized.chars().any(char::is_control)
        || looks_like_absolute_path_v1(&normalized)
    {
        return Err(Error::InvalidContract(
            "asset usage key must be a portable 1..=256 character identifier".to_owned(),
        ));
    }
    Ok(normalized)
}

fn normalize_identity_part_v1(label: &str, value: &str) -> Result<String> {
    let normalized = value.trim();
    if normalized.is_empty()
        || normalized.chars().count() > 256
        || normalized.chars().any(char::is_control)
        || looks_like_absolute_path_v1(normalized)
    {
        return Err(Error::InvalidContract(format!(
            "{label} must be a portable non-empty identifier"
        )));
    }
    Ok(normalized.to_owned())
}

fn looks_like_absolute_path_v1(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with('/')
        || trimmed.starts_with("\\")
        || (trimmed.len() >= 3
            && trimmed.as_bytes()[1] == b':'
            && matches!(trimmed.as_bytes()[2], b'\\' | b'/'))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::{Artifact, LogicalUri, Workspace};

    fn artifact(
        project_id: &str,
        job_id: &str,
        artifact_id: &str,
        uri: &str,
        sha256: &str,
        source_asset_id: &str,
        created_at: DateTime<Utc>,
    ) -> Artifact {
        Artifact {
            artifact_id: artifact_id.to_owned(),
            project_id: Some(project_id.to_owned()),
            artifact_type: "image".to_owned(),
            uri: LogicalUri::parse(uri).unwrap(),
            sha256: sha256.to_owned(),
            size_bytes: 128,
            input_hash: Some(format!("input-{artifact_id}")),
            producer_job: Some(job_id.to_owned()),
            created_at,
            metadata: json!({
                "source_provider": "pexels",
                "source_asset_id": source_asset_id,
                "width": 1280,
                "height": 720,
                "provenance": {
                    "provider": "pexels",
                    "source_id": source_asset_id,
                    "license": "pexels"
                }
            }),
        }
    }

    fn commit_fixture_artifact(
        store: &mut StateStore,
        project_id: &str,
        artifact_id: &str,
        uri: &str,
        sha256: &str,
        source_asset_id: &str,
        created_at: DateTime<Utc>,
    ) -> Artifact {
        let job = store
            .create_job(
                project_id,
                "visual.library-fixture",
                artifact_id,
                &format!("input-{artifact_id}"),
            )
            .unwrap();
        let artifact = artifact(
            project_id,
            &job.job_id,
            artifact_id,
            uri,
            sha256,
            source_asset_id,
            created_at,
        );
        store.commit_job_success(&artifact).unwrap();
        artifact
    }

    #[test]
    fn tag_normalization_is_deterministic_and_rejects_machine_paths() {
        assert_eq!(
            normalize_asset_tag_v1("  Warm   B-Roll  ").unwrap(),
            "warm b-roll"
        );
        assert!(normalize_asset_tag_v1("").is_err());
        assert!(normalize_asset_tag_v1("/Users/alice/library").is_err());
        assert!(normalize_asset_tag_v1("C:\\secret").is_err());
    }

    #[test]
    fn tags_and_usage_are_idempotent_and_sorted() {
        let root = tempdir().unwrap();
        let workspace = Workspace::create(root.path().join("data")).unwrap();
        let mut store = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = store.create_project("Library").unwrap();
        let created_at = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
        let artifact = commit_fixture_artifact(
            &mut store,
            &project.id,
            "asset-a",
            "project://visual/a.png",
            &"a".repeat(64),
            "pexels-1",
            created_at,
        );

        store.add_asset_tag_v1(&artifact.artifact_id, " Warm ").unwrap();
        store.add_asset_tag_v1(&artifact.artifact_id, "warm").unwrap();
        store.add_asset_tag_v1(&artifact.artifact_id, "Faith").unwrap();
        assert_eq!(
            store.list_asset_tags_v1(&artifact.artifact_id).unwrap(),
            vec!["faith".to_owned(), "warm".to_owned()]
        );
        assert_eq!(
            store
                .replace_asset_tags_v1(
                    &artifact.artifact_id,
                    &[" Hero ".to_owned(), "warm".to_owned(), "hero".to_owned()],
                )
                .unwrap(),
            vec!["hero".to_owned(), "warm".to_owned()]
        );
        assert_eq!(
            store.list_asset_tags_v1(&artifact.artifact_id).unwrap(),
            vec!["hero".to_owned(), "warm".to_owned()]
        );

        let later = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
        store
            .record_asset_usage_v1(
                &artifact.artifact_id,
                Some(&project.id),
                "timeline_clip",
                "clip-1",
                later,
            )
            .unwrap();
        store
            .record_asset_usage_v1(
                &artifact.artifact_id,
                Some(&project.id),
                "timeline_clip",
                "clip-1",
                later + Duration::hours(1),
            )
            .unwrap();

        let usages = store.list_asset_usage_v1(&artifact.artifact_id).unwrap();
        assert_eq!(usages.len(), 2);
        assert_eq!(usages[0].usage_kind, "timeline_clip");
        assert_eq!(usages[0].used_at, later + Duration::hours(1));
        assert_eq!(usages[1].usage_kind, "job_output");
    }

    #[test]
    fn snapshot_detects_exact_duplicates_and_source_reuse() {
        let root = tempdir().unwrap();
        let workspace = Workspace::create(root.path().join("data")).unwrap();
        let mut store = StateStore::open(workspace.sqlite_path()).unwrap();
        let first_project = store.create_project("One").unwrap();
        let second_project = store.create_project("Two").unwrap();
        let first_at = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
        let second_at = Utc.with_ymd_and_hms(2026, 9, 2, 12, 0, 0).unwrap();

        commit_fixture_artifact(
            &mut store,
            &first_project.id,
            "asset-a",
            "project://visual/a.png",
            &"b".repeat(64),
            "source-42",
            first_at,
        );
        commit_fixture_artifact(
            &mut store,
            &second_project.id,
            "asset-b",
            "project://visual/b.png",
            &"b".repeat(64),
            "source-42",
            second_at,
        );

        let reference = Utc.with_ymd_and_hms(2026, 9, 6, 12, 0, 0).unwrap();
        let snapshot = store.asset_library_snapshot_v1(reference).unwrap();

        assert_eq!(snapshot.total_assets, 2);
        assert_eq!(snapshot.duplicate_groups, 1);
        assert_eq!(snapshot.source_reuse_groups, 1);
        assert_eq!(snapshot.entries[0].asset.asset_id, "asset-b");
        assert_eq!(
            snapshot.entries[0].duplicate_asset_ids,
            vec!["asset-a".to_owned()]
        );
        assert_eq!(
            snapshot.entries[0].source_reuse_asset_ids,
            vec!["asset-a".to_owned()]
        );

        let facts = store
            .source_reuse_facts_v1("pexels", "source-42", reference)
            .unwrap();
        assert_eq!(
            facts.artifact_ids,
            vec!["asset-a".to_owned(), "asset-b".to_owned()]
        );
        assert_eq!(facts.usage_count, 2);
        assert!(facts.used_recently);
    }

    #[test]
    fn library_projection_survives_data_root_move_and_read_only_open() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        let moved = root.path().join("moved");
        let workspace = Workspace::create(&source).unwrap();
        let mut store = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = store.create_project("Portable Library").unwrap();
        let created_at = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
        let artifact = commit_fixture_artifact(
            &mut store,
            &project.id,
            "asset-portable",
            "project://visual/portable.png",
            &"c".repeat(64),
            "portable-source",
            created_at,
        );
        store.add_asset_tag_v1(&artifact.artifact_id, "portable").unwrap();
        let reference = Utc.with_ymd_and_hms(2026, 9, 6, 12, 0, 0).unwrap();
        let before = store.asset_library_snapshot_v1(reference).unwrap();
        drop(store);
        drop(workspace);

        std::fs::rename(&source, &moved).unwrap();
        let reopened = Workspace::open(&moved).unwrap();
        let read_only = StateStore::open_read_only(reopened.sqlite_path()).unwrap();
        let after = read_only.asset_library_snapshot_v1(reference).unwrap();

        assert_eq!(after, before);
        assert!(read_only
            .add_asset_tag_v1(&artifact.artifact_id, "forbidden")
            .is_err());
    }

    #[test]
    fn non_media_artifacts_are_not_exposed_as_assets() {
        let root = tempdir().unwrap();
        let workspace = Workspace::create(root.path().join("data")).unwrap();
        let mut store = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = store.create_project("Reports").unwrap();
        let job = store
            .create_job(&project.id, "report", "source", "report-input")
            .unwrap();
        let artifact = Artifact {
            artifact_id: "report-a".to_owned(),
            project_id: Some(project.id),
            artifact_type: "report".to_owned(),
            uri: LogicalUri::parse("project://reports/a.json").unwrap(),
            sha256: "d".repeat(64),
            size_bytes: 42,
            input_hash: Some("report-input".to_owned()),
            producer_job: Some(job.job_id),
            created_at: Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap(),
            metadata: json!({}),
        };
        store.commit_job_success(&artifact).unwrap();

        let snapshot = store
            .asset_library_snapshot_v1(Utc.with_ymd_and_hms(2026, 9, 6, 12, 0, 0).unwrap())
            .unwrap();
        assert!(snapshot.entries.is_empty());
    }
}
