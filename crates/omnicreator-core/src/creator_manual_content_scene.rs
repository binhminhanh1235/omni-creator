use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    deterministic_input_hash, ingest_manual_result_file_v1, load_latest_creator_content_v1,
    segment_creator_script_v1, Artifact, ArtifactStore, CreatorContentV1, CreatorInputV1,
    CreatorScenePlanV1, Error, LogicalUri, ManualResultIngestRequestV1, ManualResultOutcomeV1,
    ManualResultProvenanceV1, Result, SceneIntentV1, StateStore,
    CREATOR_CONTENT_ARTIFACT_TYPE_V1, CREATOR_CONTENT_SCHEMA_V1, CREATOR_CONTENT_VERSION_V1,
    CREATOR_SCENE_PLAN_ARTIFACT_TYPE_V1, CREATOR_SCENE_PLAN_SCHEMA_V1,
    CREATOR_SCENE_PLAN_VERSION_V1, CREATOR_STEP_CONTENT_PREPARE_V1,
    CREATOR_STEP_SCENE_PLAN_V1, CREATOR_WORKFLOW_UNIT_PROJECT_V1, MANUAL_RESULT_SCHEMA_V1,
    MANUAL_RESULT_VERSION_V1, SCENE_INTENT_SCHEMA, SCENE_INTENT_SCHEMA_VERSION,
};

pub const MANUAL_SCENE_PLAN_DRAFT_SCHEMA_V1: &str = "omnicreator.manual-scene-plan-draft";
pub const MANUAL_SCENE_PLAN_DRAFT_VERSION_V1: u32 = 1;
pub const MAX_MANUAL_TEXT_IMPORT_BYTES_V1: u64 = 8 * 1024 * 1024;
pub const MAX_MANUAL_SCENE_PLAN_IMPORT_BYTES_V1: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ManualSceneIntentDraftV1 {
    pub purpose: String,
    pub scene_type: String,
    #[serde(default)]
    pub emotion_before: Option<String>,
    #[serde(default)]
    pub emotion_after: Option<String>,
    #[serde(default)]
    pub duration_hint: Option<f64>,
    #[serde(default)]
    pub visual_ideas: Vec<String>,
    #[serde(default)]
    pub search_queries: Vec<String>,
    #[serde(default)]
    pub avoid: Vec<String>,
    #[serde(default)]
    pub continuity: BTreeMap<String, serde_json::Value>,
    pub aspect_ratio: String,
}

impl Default for ManualSceneIntentDraftV1 {
    fn default() -> Self {
        Self {
            purpose: "Support the narration beat".to_owned(),
            scene_type: "b-roll".to_owned(),
            emotion_before: None,
            emotion_after: None,
            duration_hint: None,
            visual_ideas: Vec::new(),
            search_queries: Vec::new(),
            avoid: Vec::new(),
            continuity: BTreeMap::new(),
            aspect_ratio: "16:9".to_owned(),
        }
    }
}

impl ManualSceneIntentDraftV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.purpose.trim().is_empty()
            || self.scene_type.trim().is_empty()
            || self.aspect_ratio.trim().is_empty()
        {
            return Err(Error::InvalidContract(
                "manual SceneIntent purpose, scene_type and aspect_ratio must not be empty"
                    .to_owned(),
            ));
        }
        if self.duration_hint.is_some_and(|duration| duration <= 0.0) {
            return Err(Error::InvalidContract(
                "manual SceneIntent duration_hint must be positive".to_owned(),
            ));
        }
        if self
            .visual_ideas
            .iter()
            .chain(self.search_queries.iter())
            .chain(self.avoid.iter())
            .any(|value| value.trim().is_empty())
        {
            return Err(Error::InvalidContract(
                "manual SceneIntent lists must not contain empty values".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ManualScenePlanDraftV1 {
    pub schema: String,
    pub version: u32,
    pub scenes: Vec<ManualSceneIntentDraftV1>,
}

impl ManualScenePlanDraftV1 {
    pub fn for_content_v1(content: &CreatorContentV1) -> Result<Self> {
        content.validate_v1()?;
        Ok(Self {
            schema: MANUAL_SCENE_PLAN_DRAFT_SCHEMA_V1.to_owned(),
            version: MANUAL_SCENE_PLAN_DRAFT_VERSION_V1,
            scenes: content
                .segments
                .iter()
                .map(|_| ManualSceneIntentDraftV1::default())
                .collect(),
        })
    }

    pub fn from_canonical_v1(plan: &CreatorScenePlanV1) -> Self {
        Self {
            schema: MANUAL_SCENE_PLAN_DRAFT_SCHEMA_V1.to_owned(),
            version: MANUAL_SCENE_PLAN_DRAFT_VERSION_V1,
            scenes: plan
                .scenes
                .iter()
                .map(|scene| ManualSceneIntentDraftV1 {
                    purpose: scene.purpose.clone(),
                    scene_type: scene.scene_type.clone(),
                    emotion_before: scene.emotion_before.clone(),
                    emotion_after: scene.emotion_after.clone(),
                    duration_hint: scene.duration_hint,
                    visual_ideas: scene.visual_ideas.clone(),
                    search_queries: scene.search_queries.clone(),
                    avoid: scene.avoid.clone(),
                    continuity: scene.continuity.clone(),
                    aspect_ratio: scene.aspect_ratio.clone(),
                })
                .collect(),
        }
    }

    pub fn validate_v1(&self, content: &CreatorContentV1) -> Result<()> {
        content.validate_v1()?;
        if self.schema != MANUAL_SCENE_PLAN_DRAFT_SCHEMA_V1
            || self.version != MANUAL_SCENE_PLAN_DRAFT_VERSION_V1
        {
            return Err(Error::InvalidContract(
                "unsupported manual ScenePlan draft schema/version".to_owned(),
            ));
        }
        if self.scenes.len() != content.segments.len() {
            return Err(Error::InvalidContract(format!(
                "manual ScenePlan must contain exactly {} scenes",
                content.segments.len()
            )));
        }
        for scene in &self.scenes {
            scene.validate_v1()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ManualCreatorContentOutcomeV1 {
    pub content: CreatorContentV1,
    pub artifact: Artifact,
    pub ingestion: ManualResultOutcomeV1,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ManualCreatorScenePlanOutcomeV1 {
    pub scene_plan: CreatorScenePlanV1,
    pub artifact: Artifact,
    pub ingestion: ManualResultOutcomeV1,
}

pub fn build_manual_creator_content_v1(
    project_id: &str,
    script: &str,
) -> Result<CreatorContentV1> {
    if project_id.trim().is_empty() {
        return Err(Error::InvalidContract(
            "manual creator content requires project_id".to_owned(),
        ));
    }
    let script = script.trim();
    if script.is_empty() {
        return Err(Error::InvalidContract(
            "manual creator script must not be empty".to_owned(),
        ));
    }
    let source = CreatorInputV1::script(script.to_owned());
    let content = CreatorContentV1 {
        schema: CREATOR_CONTENT_SCHEMA_V1.to_owned(),
        schema_version: CREATOR_CONTENT_VERSION_V1,
        project_id: project_id.to_owned(),
        source,
        script: script.to_owned(),
        segments: segment_creator_script_v1(script)?,
    };
    content.validate_v1()?;
    Ok(content)
}

pub fn build_manual_creator_scene_plan_v1(
    content: &CreatorContentV1,
    content_sha256: &str,
    draft: &ManualScenePlanDraftV1,
) -> Result<CreatorScenePlanV1> {
    draft.validate_v1(content)?;
    let scenes = content
        .segments
        .iter()
        .zip(&draft.scenes)
        .enumerate()
        .map(|(index, (segment, draft))| {
            let scene = SceneIntentV1 {
                schema: SCENE_INTENT_SCHEMA.to_owned(),
                schema_version: SCENE_INTENT_SCHEMA_VERSION,
                id: format!("SC{:03}", index + 1),
                segment_id: segment.id.clone(),
                narration: segment.text.clone(),
                purpose: draft.purpose.trim().to_owned(),
                scene_type: draft.scene_type.trim().to_owned(),
                emotion_before: normalized_optional_v1(draft.emotion_before.as_deref()),
                emotion_after: normalized_optional_v1(draft.emotion_after.as_deref()),
                duration_hint: draft.duration_hint,
                visual_ideas: normalized_list_v1(&draft.visual_ideas),
                search_queries: normalized_list_v1(&draft.search_queries),
                avoid: normalized_list_v1(&draft.avoid),
                continuity: draft.continuity.clone(),
                aspect_ratio: draft.aspect_ratio.trim().to_owned(),
            };
            scene.validate_v1()?;
            Ok(scene)
        })
        .collect::<Result<Vec<_>>>()?;

    let plan = CreatorScenePlanV1 {
        schema: CREATOR_SCENE_PLAN_SCHEMA_V1.to_owned(),
        schema_version: CREATOR_SCENE_PLAN_VERSION_V1,
        project_id: content.project_id.clone(),
        content_sha256: content_sha256.to_owned(),
        scenes,
    };
    plan.validate_v1(content)?;
    Ok(plan)
}

pub fn parse_portable_manual_scene_plan_v1(
    bytes: &[u8],
    content: &CreatorContentV1,
    content_sha256: &str,
) -> Result<CreatorScenePlanV1> {
    if let Ok(draft) = serde_json::from_slice::<ManualScenePlanDraftV1>(bytes) {
        return build_manual_creator_scene_plan_v1(content, content_sha256, &draft);
    }
    let imported: CreatorScenePlanV1 = serde_json::from_slice(bytes).map_err(|error| {
        Error::InvalidContract(format!(
            "manual ScenePlan import must be a portable draft or CreatorScenePlanV1 JSON: {error}"
        ))
    })?;
    let draft = ManualScenePlanDraftV1::from_canonical_v1(&imported);
    build_manual_creator_scene_plan_v1(content, content_sha256, &draft)
}

pub fn provide_manual_creator_content_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    script: &str,
    provenance: ManualResultProvenanceV1,
) -> Result<ManualCreatorContentOutcomeV1> {
    state_store.get_project(project_id)?;
    let content = build_manual_creator_content_v1(project_id, script)?;
    let bytes = serde_json::to_vec(&content)?;
    let target_uri = manual_target_uri_v1("content", "json")?;
    let request = ManualResultIngestRequestV1 {
        schema: MANUAL_RESULT_SCHEMA_V1.to_owned(),
        version: MANUAL_RESULT_VERSION_V1,
        project_id: project_id.to_owned(),
        workflow_step: CREATOR_STEP_CONTENT_PREPARE_V1.to_owned(),
        workflow_unit: CREATOR_WORKFLOW_UNIT_PROJECT_V1.to_owned(),
        job_step: CREATOR_STEP_CONTENT_PREPARE_V1.to_owned(),
        job_unit: CREATOR_WORKFLOW_UNIT_PROJECT_V1.to_owned(),
        artifact_type: CREATOR_CONTENT_ARTIFACT_TYPE_V1.to_owned(),
        target_uri,
        provenance,
        stage_metadata: serde_json::json!({
            "contract": CREATOR_CONTENT_SCHEMA_V1,
            "schema_version": CREATOR_CONTENT_VERSION_V1,
            "mode": "manual",
        }),
        replace_existing: true,
        complete_workflow_step: true,
    };
    let ingestion = ingest_bytes_via_manual_foundation_v1(
        state_store,
        artifact_store,
        &request,
        &bytes,
        "content",
    )?;
    let artifact = ingestion.artifact.clone();
    Ok(ManualCreatorContentOutcomeV1 {
        content,
        artifact,
        ingestion,
    })
}

pub fn provide_manual_creator_scene_plan_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    draft: &ManualScenePlanDraftV1,
    provenance: ManualResultProvenanceV1,
) -> Result<ManualCreatorScenePlanOutcomeV1> {
    state_store.get_project(project_id)?;
    let (content, content_artifact) = load_latest_creator_content_v1(
        state_store,
        artifact_store,
        project_id,
    )?
    .ok_or_else(|| {
        Error::InvalidContract(
            "manual ScenePlan requires a verified canonical creator content artifact".to_owned(),
        )
    })?;
    let scene_plan =
        build_manual_creator_scene_plan_v1(&content, &content_artifact.sha256, draft)?;
    persist_manual_scene_plan_v1(
        state_store,
        artifact_store,
        project_id,
        scene_plan,
        provenance,
    )
}

pub fn import_manual_creator_script_file_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    path: impl AsRef<Path>,
) -> Result<ManualCreatorContentOutcomeV1> {
    let path = path.as_ref();
    validate_extension_v1(path, &["txt", "md", "markdown"], "manual script")?;
    let bytes = read_limited_v1(path, MAX_MANUAL_TEXT_IMPORT_BYTES_V1, "manual script")?;
    let script = std::str::from_utf8(&bytes).map_err(|_| {
        Error::InvalidContract("manual script import must be UTF-8 text".to_owned())
    })?;
    provide_manual_creator_content_v1(
        state_store,
        artifact_store,
        project_id,
        script,
        ManualResultProvenanceV1::local_file(),
    )
}

pub fn import_manual_creator_scene_plan_file_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    path: impl AsRef<Path>,
) -> Result<ManualCreatorScenePlanOutcomeV1> {
    let path = path.as_ref();
    validate_extension_v1(path, &["json"], "manual ScenePlan")?;
    let bytes = read_limited_v1(
        path,
        MAX_MANUAL_SCENE_PLAN_IMPORT_BYTES_V1,
        "manual ScenePlan",
    )?;
    let (content, content_artifact) = load_latest_creator_content_v1(
        state_store,
        artifact_store,
        project_id,
    )?
    .ok_or_else(|| {
        Error::InvalidContract(
            "manual ScenePlan import requires verified creator content".to_owned(),
        )
    })?;
    let scene_plan =
        parse_portable_manual_scene_plan_v1(&bytes, &content, &content_artifact.sha256)?;
    persist_manual_scene_plan_v1(
        state_store,
        artifact_store,
        project_id,
        scene_plan,
        ManualResultProvenanceV1::local_file(),
    )
}

fn persist_manual_scene_plan_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    scene_plan: CreatorScenePlanV1,
    provenance: ManualResultProvenanceV1,
) -> Result<ManualCreatorScenePlanOutcomeV1> {
    let bytes = serde_json::to_vec(&scene_plan)?;
    let request = ManualResultIngestRequestV1 {
        schema: MANUAL_RESULT_SCHEMA_V1.to_owned(),
        version: MANUAL_RESULT_VERSION_V1,
        project_id: project_id.to_owned(),
        workflow_step: CREATOR_STEP_SCENE_PLAN_V1.to_owned(),
        workflow_unit: CREATOR_WORKFLOW_UNIT_PROJECT_V1.to_owned(),
        job_step: CREATOR_STEP_SCENE_PLAN_V1.to_owned(),
        job_unit: CREATOR_WORKFLOW_UNIT_PROJECT_V1.to_owned(),
        artifact_type: CREATOR_SCENE_PLAN_ARTIFACT_TYPE_V1.to_owned(),
        target_uri: manual_target_uri_v1("scene-plan", "json")?,
        provenance,
        stage_metadata: serde_json::json!({
            "contract": CREATOR_SCENE_PLAN_SCHEMA_V1,
            "schema_version": CREATOR_SCENE_PLAN_VERSION_V1,
            "mode": "manual",
            "content_sha256": &scene_plan.content_sha256,
            "scene_count": scene_plan.scenes.len(),
        }),
        replace_existing: true,
        complete_workflow_step: true,
    };
    let ingestion = ingest_bytes_via_manual_foundation_v1(
        state_store,
        artifact_store,
        &request,
        &bytes,
        "scene-plan",
    )?;
    let artifact = ingestion.artifact.clone();
    Ok(ManualCreatorScenePlanOutcomeV1 {
        scene_plan,
        artifact,
        ingestion,
    })
}

fn ingest_bytes_via_manual_foundation_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    request: &ManualResultIngestRequestV1,
    bytes: &[u8],
    kind: &str,
) -> Result<ManualResultOutcomeV1> {
    let staging = staging_path_v1(artifact_store, kind)?;
    if let Some(parent) = staging.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&staging, bytes)?;
    let result = ingest_manual_result_file_v1(state_store, artifact_store, request, &staging);
    let _ = fs::remove_file(&staging);
    result
}

fn staging_path_v1(artifact_store: &ArtifactStore, kind: &str) -> Result<PathBuf> {
    if kind.trim().is_empty() || kind.contains('/') || kind.contains('\\') {
        return Err(Error::InvalidContract(
            "manual staging kind must be symbolic".to_owned(),
        ));
    }
    Ok(artifact_store
        .data_root()
        .join(".omnicreator")
        .join("manual-staging")
        .join(format!("{kind}-{}.tmp", Uuid::new_v4().simple())))
}

fn manual_target_uri_v1(kind: &str, extension: &str) -> Result<LogicalUri> {
    let id = Uuid::new_v4().simple().to_string();
    LogicalUri::parse(&format!(
        "project://manual-results/{kind}/{id}.{extension}"
    ))
}

fn read_limited_v1(path: &Path, max_bytes: u64, label: &str) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(Error::InvalidContract(format!(
            "{label} import must be a regular file"
        )));
    }
    if metadata.len() > max_bytes {
        return Err(Error::InvalidContract(format!(
            "{label} import exceeds the {max_bytes}-byte safety limit"
        )));
    }
    Ok(fs::read(path)?)
}

fn validate_extension_v1(path: &Path, allowed: &[&str], label: &str) -> Result<()> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| Error::InvalidContract(format!("{label} import has no file extension")))?;
    if !allowed.iter().any(|allowed| *allowed == extension) {
        return Err(Error::InvalidContract(format!(
            "{label} import extension .{extension} is not supported"
        )));
    }
    Ok(())
}

fn normalized_optional_v1(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn normalized_list_v1(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn manual_scene_plan_fingerprint_v1(draft: &ManualScenePlanDraftV1) -> Result<String> {
    let json = serde_json::to_vec(draft)?;
    Ok(deterministic_input_hash(&[
        b"manual-scene-plan-draft-v1",
        json.as_slice(),
    ]))
}
