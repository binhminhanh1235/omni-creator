use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    artifact_store::{AttemptOutputPromotion, AttemptPromotionRequest},
    deterministic_input_hash, Artifact, ArtifactStore, CreatorContentOptions, Error,
    LlmGatewayClient, LogicalUri, Result, SceneIntentGenerationOptions, SceneIntentV1, SegmentV1,
    StateStore, StepStatus, VoiceDirectionV1, WorkflowStep, CREATOR_STEP_CONTENT_PREPARE_V1,
    CREATOR_STEP_SCENE_PLAN_V1, CREATOR_WORKFLOW_UNIT_PROJECT_V1, SCENE_INTENT_SCHEMA,
    SCENE_INTENT_SCHEMA_VERSION, SEGMENT_SCHEMA, SEGMENT_SCHEMA_VERSION,
};

pub const CREATOR_INPUT_SCHEMA_V1: &str = "omnicreator.creator-input";
pub const CREATOR_INPUT_VERSION_V1: u32 = 1;
pub const CREATOR_CONTENT_SCHEMA_V1: &str = "omnicreator.creator-content";
pub const CREATOR_CONTENT_VERSION_V1: u32 = 1;
pub const CREATOR_SCENE_PLAN_SCHEMA_V1: &str = "omnicreator.creator-scene-plan";
pub const CREATOR_SCENE_PLAN_VERSION_V1: u32 = 1;

pub const CREATOR_CONTENT_ARTIFACT_TYPE_V1: &str = "creator-content";
pub const CREATOR_SCENE_PLAN_ARTIFACT_TYPE_V1: &str = "creator-scene-plan";
pub const LLMGATEWAY_SETUP_REQUIRED_ERROR_V1: &str = "LLMGATEWAY_SETUP_REQUIRED";

const CREATOR_CONTENT_WORKER_V1: &str = "creator-content-v1";
const CREATOR_SCENE_WORKER_V1: &str = "llmgateway-scene-intelligence-v1";
const CREATOR_SCRIPT_INSTRUCTION_V1: &str = "Write a production-ready narration script from the supplied topic. Preserve the creator's core intent, use clear spoken language, and separate meaningful visual beats with blank lines. Return only the narration script, without markdown fences, provider names, routing details, or production metadata.";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CreatorInputKindV1 {
    Topic,
    Script,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreatorInputV1 {
    pub schema: String,
    pub schema_version: u32,
    pub kind: CreatorInputKindV1,
    pub text: String,
}

impl CreatorInputV1 {
    pub fn topic(text: impl Into<String>) -> Self {
        Self {
            schema: CREATOR_INPUT_SCHEMA_V1.to_owned(),
            schema_version: CREATOR_INPUT_VERSION_V1,
            kind: CreatorInputKindV1::Topic,
            text: text.into(),
        }
    }

    pub fn script(text: impl Into<String>) -> Self {
        Self {
            schema: CREATOR_INPUT_SCHEMA_V1.to_owned(),
            schema_version: CREATOR_INPUT_VERSION_V1,
            kind: CreatorInputKindV1::Script,
            text: text.into(),
        }
    }

    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != CREATOR_INPUT_SCHEMA_V1 || self.schema_version != CREATOR_INPUT_VERSION_V1
        {
            return Err(Error::InvalidContract(
                "unsupported creator input schema/version".to_owned(),
            ));
        }
        if self.text.trim().is_empty() {
            return Err(Error::InvalidContract(
                "creator input text must not be empty".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn canonical_json_v1(&self) -> Result<String> {
        self.validate_v1()?;
        Ok(serde_json::to_string(self)?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CreatorContentV1 {
    pub schema: String,
    pub schema_version: u32,
    pub project_id: String,
    pub source: CreatorInputV1,
    pub script: String,
    pub segments: Vec<SegmentV1>,
}

impl CreatorContentV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != CREATOR_CONTENT_SCHEMA_V1
            || self.schema_version != CREATOR_CONTENT_VERSION_V1
        {
            return Err(Error::InvalidContract(
                "unsupported creator content schema/version".to_owned(),
            ));
        }
        if self.project_id.trim().is_empty() || self.script.trim().is_empty() {
            return Err(Error::InvalidContract(
                "creator content project_id and script must not be empty".to_owned(),
            ));
        }
        self.source.validate_v1()?;
        if self.segments.is_empty() {
            return Err(Error::InvalidContract(
                "creator content must contain at least one segment".to_owned(),
            ));
        }
        for (index, segment) in self.segments.iter().enumerate() {
            segment.validate_v1()?;
            let expected_order = u32::try_from(index + 1)
                .map_err(|_| Error::InvalidContract("too many creator segments".to_owned()))?;
            let expected_id = format!("S{expected_order:03}");
            if segment.order != expected_order || segment.id != expected_id {
                return Err(Error::InvalidContract(format!(
                    "creator segment {} must be {} at order {}",
                    segment.id, expected_id, expected_order
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CreatorScenePlanV1 {
    pub schema: String,
    pub schema_version: u32,
    pub project_id: String,
    pub content_sha256: String,
    pub scenes: Vec<SceneIntentV1>,
}

impl CreatorScenePlanV1 {
    pub fn validate_v1(&self, content: &CreatorContentV1) -> Result<()> {
        if self.schema != CREATOR_SCENE_PLAN_SCHEMA_V1
            || self.schema_version != CREATOR_SCENE_PLAN_VERSION_V1
        {
            return Err(Error::InvalidContract(
                "unsupported creator scene plan schema/version".to_owned(),
            ));
        }
        if self.project_id != content.project_id {
            return Err(Error::InvalidContract(
                "creator scene plan project_id must match content".to_owned(),
            ));
        }
        if !is_sha256_hex_v1(&self.content_sha256) {
            return Err(Error::InvalidContract(
                "creator scene plan content_sha256 must be lowercase SHA-256 hex".to_owned(),
            ));
        }
        if self.scenes.len() != content.segments.len() {
            return Err(Error::InvalidContract(
                "creator scene plan must contain exactly one scene per content segment".to_owned(),
            ));
        }
        for (index, (scene, segment)) in self.scenes.iter().zip(&content.segments).enumerate() {
            scene.validate_v1()?;
            let expected_id = format!("SC{:03}", index + 1);
            if scene.id != expected_id
                || scene.segment_id != segment.id
                || scene.narration != segment.text
            {
                return Err(Error::InvalidContract(format!(
                    "creator scene {} does not preserve its canonical segment identity/narration",
                    scene.id
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreatorContentSceneOptionsV1 {
    pub aspect_ratio: String,
    #[serde(default)]
    pub visual_rules: Vec<String>,
    #[serde(default)]
    pub avoid: Vec<String>,
}

impl Default for CreatorContentSceneOptionsV1 {
    fn default() -> Self {
        Self {
            aspect_ratio: "16:9".to_owned(),
            visual_rules: Vec::new(),
            avoid: Vec::new(),
        }
    }
}

impl CreatorContentSceneOptionsV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.aspect_ratio.trim().is_empty() {
            return Err(Error::InvalidContract(
                "creator scene aspect_ratio must not be empty".to_owned(),
            ));
        }
        if self
            .visual_rules
            .iter()
            .chain(self.avoid.iter())
            .any(|value| value.trim().is_empty())
        {
            return Err(Error::InvalidContract(
                "creator scene rules must not contain empty entries".to_owned(),
            ));
        }
        Ok(())
    }

    fn generation_options_v1(&self) -> SceneIntentGenerationOptions {
        SceneIntentGenerationOptions {
            aspect_ratio: self.aspect_ratio.clone(),
            model: None,
            max_attempts: 3,
            max_tokens: Some(1_200),
            visual_rules: self.visual_rules.clone(),
            avoid: self.avoid.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CreatorContentSceneOutcomeV1 {
    pub content: CreatorContentV1,
    pub scene_plan: CreatorScenePlanV1,
    pub content_artifact: Artifact,
    pub scene_plan_artifact: Artifact,
    pub content_cache_hit: bool,
    pub scene_plan_cache_hit: bool,
}

pub trait CreatorLlmExecutorV1 {
    fn create_script_v1(&self, input: &CreatorInputV1) -> Result<String>;

    fn create_scene_intent_v1(
        &self,
        segment: &SegmentV1,
        scene_id: &str,
        options: &CreatorContentSceneOptionsV1,
    ) -> Result<SceneIntentV1>;
}

impl CreatorLlmExecutorV1 for LlmGatewayClient {
    fn create_script_v1(&self, input: &CreatorInputV1) -> Result<String> {
        input.validate_v1()?;
        match input.kind {
            CreatorInputKindV1::Script => Ok(input.text.trim().to_owned()),
            CreatorInputKindV1::Topic => {
                let result = self.run_content_task(
                    CREATOR_SCRIPT_INSTRUCTION_V1,
                    input.text.trim(),
                    &CreatorContentOptions::default(),
                )?;
                if result.content.trim().is_empty() {
                    return Err(Error::InvalidStructuredOutput {
                        contract: CREATOR_CONTENT_SCHEMA_V1.to_owned(),
                        attempts: 1,
                        reason: "LLMGateway returned an empty creator script".to_owned(),
                    });
                }
                Ok(result.content.trim().to_owned())
            }
        }
    }

    fn create_scene_intent_v1(
        &self,
        segment: &SegmentV1,
        scene_id: &str,
        options: &CreatorContentSceneOptionsV1,
    ) -> Result<SceneIntentV1> {
        options.validate_v1()?;
        self.generate_scene_intent(segment, scene_id, &options.generation_options_v1())
    }
}

pub fn run_creator_content_scene_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    llm: &impl CreatorLlmExecutorV1,
    project_id: &str,
    input: &CreatorInputV1,
    options: &CreatorContentSceneOptionsV1,
) -> Result<CreatorContentSceneOutcomeV1> {
    input.validate_v1()?;
    options.validate_v1()?;
    let project = state_store.get_project(project_id)?;
    if !matches!(
        project.studio_pack.as_deref(),
        Some(studio_pack) if !studio_pack.is_empty()
    ) {
        return Err(Error::InvalidContract(
            "creator content orchestration requires a Project bound to a Studio Pack".to_owned(),
        ));
    }

    let (content_step, scene_step) = require_creator_p0_steps_v1(state_store, project_id)?;

    let content_hash = creator_content_input_hash_v1(&project.id, project.script_version, input)?;
    let (content, content_artifact, content_cache_hit) = run_content_stage_v1(
        state_store,
        artifact_store,
        llm,
        &content_step,
        input,
        &content_hash,
    )?;

    state_store.refresh_ready_steps(project_id)?;
    let scene_step = state_store.get_step(&scene_step.step_id)?;
    if scene_step.status == StepStatus::NotReady {
        return Err(Error::InvalidTransition(
            "scene.plan remained NOT_READY after content.prepare succeeded".to_owned(),
        ));
    }

    let scene_hash = creator_scene_input_hash_v1(&content_artifact, options)?;
    let (scene_plan, scene_plan_artifact, scene_plan_cache_hit) = run_scene_stage_v1(
        state_store,
        artifact_store,
        llm,
        &scene_step,
        SceneStageInputV1 {
            content: &content,
            content_artifact: &content_artifact,
            options,
            input_hash: &scene_hash,
        },
    )?;

    state_store.refresh_ready_steps(project_id)?;

    Ok(CreatorContentSceneOutcomeV1 {
        content,
        scene_plan,
        content_artifact,
        scene_plan_artifact,
        content_cache_hit,
        scene_plan_cache_hit,
    })
}

fn run_content_stage_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    llm: &impl CreatorLlmExecutorV1,
    step: &WorkflowStep,
    input: &CreatorInputV1,
    input_hash: &str,
) -> Result<(CreatorContentV1, Artifact, bool)> {
    if let Some(artifact) = find_project_cache_v1(
        state_store,
        artifact_store,
        &step.project_id,
        CREATOR_STEP_CONTENT_PREPARE_V1,
        input_hash,
    )? {
        let content: CreatorContentV1 = read_json_artifact_v1(artifact_store, &artifact)?;
        content.validate_v1()?;
        mark_step_succeeded_v1(state_store, &step.step_id)?;
        return Ok((content, artifact, true));
    }

    prepare_step_for_new_input_v1(state_store, step)?;
    let job = get_or_create_job_v1(
        state_store,
        &step.project_id,
        CREATOR_STEP_CONTENT_PREPARE_V1,
        CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        input_hash,
    )?;
    state_store.set_step_status(&step.step_id, StepStatus::Running)?;
    let attempt = state_store.start_attempt(&job.job_id, Some(CREATOR_CONTENT_WORKER_V1))?;

    let result = (|| {
        let script = llm.create_script_v1(input)?;
        if script.trim().is_empty() {
            return Err(Error::InvalidContract(
                "creator script must not be empty".to_owned(),
            ));
        }
        let content = CreatorContentV1 {
            schema: CREATOR_CONTENT_SCHEMA_V1.to_owned(),
            schema_version: CREATOR_CONTENT_VERSION_V1,
            project_id: step.project_id.clone(),
            source: input.clone(),
            segments: segment_script_v1(&script)?,
            script,
        };
        content.validate_v1()?;

        let artifact = promote_json_for_attempt_v1(
            artifact_store,
            state_store,
            CreatorJsonPromotionV1 {
                attempt_id: &attempt.attempt_id,
                job_id: &job.job_id,
                value: &content,
                target_uri: content_target_uri_v1(&job.job_id)?,
                artifact_type: CREATOR_CONTENT_ARTIFACT_TYPE_V1,
                metadata: serde_json::json!({
                    "schema": CREATOR_CONTENT_SCHEMA_V1,
                    "schema_version": CREATOR_CONTENT_VERSION_V1,
                    "stage": CREATOR_STEP_CONTENT_PREPARE_V1,
                    "input_kind": input.kind,
                }),
            },
        )?;
        Ok((content, artifact))
    })();

    match result {
        Ok((content, artifact)) => {
            state_store.set_step_status(&step.step_id, StepStatus::Succeeded)?;
            Ok((content, artifact, false))
        }
        Err(error) => {
            finish_creator_failure_v1(state_store, &step.step_id, &attempt.attempt_id, &error)?;
            Err(error)
        }
    }
}

struct SceneStageInputV1<'a> {
    content: &'a CreatorContentV1,
    content_artifact: &'a Artifact,
    options: &'a CreatorContentSceneOptionsV1,
    input_hash: &'a str,
}

fn run_scene_stage_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    llm: &impl CreatorLlmExecutorV1,
    step: &WorkflowStep,
    input: SceneStageInputV1<'_>,
) -> Result<(CreatorScenePlanV1, Artifact, bool)> {
    let SceneStageInputV1 {
        content,
        content_artifact,
        options,
        input_hash,
    } = input;
    if let Some(artifact) = find_project_cache_v1(
        state_store,
        artifact_store,
        &step.project_id,
        CREATOR_STEP_SCENE_PLAN_V1,
        input_hash,
    )? {
        let scene_plan: CreatorScenePlanV1 = read_json_artifact_v1(artifact_store, &artifact)?;
        scene_plan.validate_v1(content)?;
        mark_step_succeeded_v1(state_store, &step.step_id)?;
        return Ok((scene_plan, artifact, true));
    }

    prepare_step_for_new_input_v1(state_store, step)?;
    let job = get_or_create_job_v1(
        state_store,
        &step.project_id,
        CREATOR_STEP_SCENE_PLAN_V1,
        CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        input_hash,
    )?;
    state_store.set_step_status(&step.step_id, StepStatus::Running)?;
    let attempt = state_store.start_attempt(&job.job_id, Some(CREATOR_SCENE_WORKER_V1))?;

    let result = (|| {
        let mut scenes = Vec::with_capacity(content.segments.len());
        for (index, segment) in content.segments.iter().enumerate() {
            let scene_id = format!("SC{:03}", index + 1);
            let scene = llm.create_scene_intent_v1(segment, &scene_id, options)?;
            if scene.schema != SCENE_INTENT_SCHEMA
                || scene.schema_version != SCENE_INTENT_SCHEMA_VERSION
            {
                return Err(Error::InvalidContract(format!(
                    "scene {scene_id} returned unsupported SceneIntent schema/version"
                )));
            }
            if scene.id != scene_id
                || scene.segment_id != segment.id
                || scene.narration != segment.text
            {
                return Err(Error::InvalidContract(format!(
                    "scene {scene_id} failed canonical identity preservation"
                )));
            }
            scene.validate_v1()?;
            scenes.push(scene);
        }

        let scene_plan = CreatorScenePlanV1 {
            schema: CREATOR_SCENE_PLAN_SCHEMA_V1.to_owned(),
            schema_version: CREATOR_SCENE_PLAN_VERSION_V1,
            project_id: step.project_id.clone(),
            content_sha256: content_artifact.sha256.clone(),
            scenes,
        };
        scene_plan.validate_v1(content)?;

        let artifact = promote_json_for_attempt_v1(
            artifact_store,
            state_store,
            CreatorJsonPromotionV1 {
                attempt_id: &attempt.attempt_id,
                job_id: &job.job_id,
                value: &scene_plan,
                target_uri: scene_target_uri_v1(&job.job_id)?,
                artifact_type: CREATOR_SCENE_PLAN_ARTIFACT_TYPE_V1,
                metadata: serde_json::json!({
                    "schema": CREATOR_SCENE_PLAN_SCHEMA_V1,
                    "schema_version": CREATOR_SCENE_PLAN_VERSION_V1,
                    "stage": CREATOR_STEP_SCENE_PLAN_V1,
                    "scene_count": scene_plan.scenes.len(),
                }),
            },
        )?;
        Ok((scene_plan, artifact))
    })();

    match result {
        Ok((scene_plan, artifact)) => {
            state_store.set_step_status(&step.step_id, StepStatus::Succeeded)?;
            Ok((scene_plan, artifact, false))
        }
        Err(error) => {
            finish_creator_failure_v1(state_store, &step.step_id, &attempt.attempt_id, &error)?;
            Err(error)
        }
    }
}

fn require_creator_p0_steps_v1(
    state_store: &StateStore,
    project_id: &str,
) -> Result<(WorkflowStep, WorkflowStep)> {
    let steps = state_store.list_project_steps(project_id)?;
    let content = find_project_step_v1(&steps, CREATOR_STEP_CONTENT_PREPARE_V1)?;
    let scene = find_project_step_v1(&steps, CREATOR_STEP_SCENE_PLAN_V1)?;
    Ok((content, scene))
}

fn find_project_step_v1(steps: &[WorkflowStep], key: &str) -> Result<WorkflowStep> {
    steps
        .iter()
        .find(|step| step.step == key && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1)
        .cloned()
        .ok_or_else(|| {
            Error::InvalidContract(format!(
                "creator workflow step {key}/{} is missing; materialize Phase 15 P0 first",
                CREATOR_WORKFLOW_UNIT_PROJECT_V1
            ))
        })
}

fn creator_content_input_hash_v1(
    project_id: &str,
    script_version: i64,
    input: &CreatorInputV1,
) -> Result<String> {
    let input_json = input.canonical_json_v1()?;
    Ok(deterministic_input_hash(&[
        b"creator-content-input-v1",
        project_id.as_bytes(),
        script_version.to_string().as_bytes(),
        input_json.as_bytes(),
    ]))
}

fn creator_scene_input_hash_v1(
    content_artifact: &Artifact,
    options: &CreatorContentSceneOptionsV1,
) -> Result<String> {
    options.validate_v1()?;
    if !is_sha256_hex_v1(&content_artifact.sha256) {
        return Err(Error::InvalidArtifact(
            "creator content artifact SHA-256 is invalid".to_owned(),
        ));
    }
    let options_json = serde_json::to_string(options)?;
    Ok(deterministic_input_hash(&[
        b"creator-scene-input-v1",
        content_artifact.sha256.as_bytes(),
        options_json.as_bytes(),
    ]))
}

fn segment_script_v1(script: &str) -> Result<Vec<SegmentV1>> {
    let normalized = script.replace("\r\n", "\n").replace('\r', "\n");
    let mut chunks = normalized
        .split("\n\n")
        .map(|paragraph| {
            paragraph
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>();

    if chunks.len() == 1 {
        let sentence_chunks = split_sentences_v1(&chunks[0]);
        if sentence_chunks.len() > 1 {
            chunks = sentence_chunks;
        }
    }

    if chunks.is_empty() {
        return Err(Error::InvalidContract(
            "creator script produced no non-empty segments".to_owned(),
        ));
    }

    chunks
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            let order = u32::try_from(index + 1)
                .map_err(|_| Error::InvalidContract("too many creator segments".to_owned()))?;
            Ok(SegmentV1 {
                schema: SEGMENT_SCHEMA.to_owned(),
                schema_version: SEGMENT_SCHEMA_VERSION,
                id: format!("S{order:03}"),
                order,
                text,
                voice_direction: VoiceDirectionV1::default(),
            })
        })
        .collect()
}

fn split_sentences_v1(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if matches!(ch, '.' | '!' | '?') {
            let end = index + ch.len_utf8();
            let sentence = text[start..end].trim();
            if !sentence.is_empty() {
                sentences.push(sentence.to_owned());
            }
            start = end;
        }
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        sentences.push(tail.to_owned());
    }
    if sentences.is_empty() && !text.trim().is_empty() {
        sentences.push(text.trim().to_owned());
    }
    sentences
}

fn find_project_cache_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    step_key: &str,
    input_hash: &str,
) -> Result<Option<Artifact>> {
    for job in state_store.list_project_jobs(project_id)? {
        if job.step != step_key
            || job.unit != CREATOR_WORKFLOW_UNIT_PROJECT_V1
            || job.input_hash != input_hash
            || job.status != StepStatus::Succeeded
        {
            continue;
        }
        let Some(artifact_id) = job.selected_artifact.as_deref() else {
            continue;
        };
        let artifact = state_store.get_artifact(artifact_id)?;
        if artifact.project_id.as_deref() != Some(project_id)
            || artifact.input_hash.as_deref() != Some(input_hash)
        {
            continue;
        }
        if artifact_store.verify_artifact(&artifact)? {
            return Ok(Some(artifact));
        }
    }
    Ok(None)
}

fn get_or_create_job_v1(
    state_store: &mut StateStore,
    project_id: &str,
    step_key: &str,
    unit: &str,
    input_hash: &str,
) -> Result<crate::Job> {
    let matching = state_store
        .list_project_jobs(project_id)?
        .into_iter()
        .find(|job| job.step == step_key && job.unit == unit && job.input_hash == input_hash);

    match matching {
        Some(job) if matches!(job.status, StepStatus::Retryable | StepStatus::Failed) => {
            state_store.prepare_job_retry(&job.job_id)
        }
        Some(job) if job.status == StepStatus::Ready => Ok(job),
        Some(job) if job.status == StepStatus::Fatal => Err(Error::InvalidJobState(format!(
            "creator job {} is FATAL for unchanged input; change input or resolve the blocking condition",
            job.job_id
        ))),
        Some(job) if matches!(job.status, StepStatus::Running | StepStatus::Queued) => {
            Err(Error::InvalidJobState(format!(
                "creator job {} is already active for unchanged input",
                job.job_id
            )))
        }
        _ => state_store.create_job(project_id, step_key, unit, input_hash),
    }
}

fn prepare_step_for_new_input_v1(state_store: &mut StateStore, step: &WorkflowStep) -> Result<()> {
    let current = state_store.get_step(&step.step_id)?;
    if current.status == StepStatus::Succeeded {
        let impact = state_store.invalidate_from(&step.step_id, None)?;
        for affected in impact {
            let now = state_store.get_step(&affected.step_id)?;
            if now.status != StepStatus::Stale {
                continue;
            }
            let next = if affected.step_id == step.step_id {
                StepStatus::Ready
            } else {
                StepStatus::NotReady
            };
            state_store.set_step_status(&affected.step_id, next)?;
        }
        return Ok(());
    }

    match current.status {
        StepStatus::Stale | StepStatus::Retryable | StepStatus::Failed | StepStatus::Cancelled => {
            state_store.set_step_status(&step.step_id, StepStatus::Ready)?;
        }
        StepStatus::Ready => {}
        StepStatus::NotReady => {
            return Err(Error::InvalidTransition(format!(
                "creator workflow step {} is still waiting on dependencies",
                step.step
            )));
        }
        StepStatus::Fatal => {
            return Err(Error::InvalidTransition(format!(
                "creator workflow step {} is FATAL",
                step.step
            )));
        }
        StepStatus::Running | StepStatus::Queued => {
            return Err(Error::InvalidTransition(format!(
                "creator workflow step {} is already active",
                step.step
            )));
        }
        StepStatus::Skipped => {
            state_store.set_step_status(&step.step_id, StepStatus::Ready)?;
        }
        StepStatus::Succeeded => unreachable!(),
    }
    Ok(())
}

fn mark_step_succeeded_v1(state_store: &StateStore, step_id: &str) -> Result<()> {
    let mut current = state_store.get_step(step_id)?;
    if current.status == StepStatus::Succeeded {
        return Ok(());
    }
    if matches!(
        current.status,
        StepStatus::Stale | StepStatus::NotReady | StepStatus::Skipped | StepStatus::Cancelled
    ) {
        current = state_store.set_step_status(step_id, StepStatus::Ready)?;
    }
    if matches!(
        current.status,
        StepStatus::Ready | StepStatus::Running | StepStatus::Retryable
    ) {
        state_store.set_step_status(step_id, StepStatus::Succeeded)?;
        return Ok(());
    }
    Err(Error::InvalidTransition(format!(
        "workflow step {step_id} cannot reuse a verified creator cache from {}",
        current.status.as_str()
    )))
}

fn finish_creator_failure_v1(
    state_store: &mut StateStore,
    step_id: &str,
    attempt_id: &str,
    error: &Error,
) -> Result<()> {
    let code = creator_error_code_v1(error);
    let attempt = state_store.finish_attempt_failure(attempt_id, code)?;
    let current = state_store.get_step(step_id)?;
    if current.status == StepStatus::Running {
        state_store.set_step_status(step_id, attempt.status)?;
    }
    Ok(())
}

fn creator_error_code_v1(error: &Error) -> &'static str {
    match error {
        Error::MissingLlmGatewayCredential(_) | Error::InvalidLlmGatewayConfig(_) => {
            LLMGATEWAY_SETUP_REQUIRED_ERROR_V1
        }
        Error::LlmGatewayTransport(_) => LLMGATEWAY_SETUP_REQUIRED_ERROR_V1,
        Error::LlmGatewayApi { status: 429, .. } => "RATE_LIMITED",
        Error::LlmGatewayApi { .. } => "PROVIDER_UNAVAILABLE",
        Error::InvalidStructuredOutput { .. } => "INVALID_LLM_OUTPUT",
        Error::InvalidLlmGatewayResponse(_) => "INVALID_LLM_OUTPUT",
        _ => "LOCAL_RUNTIME_CONTEXT_ERROR",
    }
}

struct CreatorJsonPromotionV1<'a, T> {
    attempt_id: &'a str,
    job_id: &'a str,
    value: &'a T,
    target_uri: LogicalUri,
    artifact_type: &'a str,
    metadata: serde_json::Value,
}

fn promote_json_for_attempt_v1<T: Serialize>(
    artifact_store: &ArtifactStore,
    state_store: &mut StateStore,
    promotion: CreatorJsonPromotionV1<'_, T>,
) -> Result<Artifact> {
    let staging = write_staging_json_v1(artifact_store, promotion.value)?;
    let result = artifact_store.promote_attempt_outputs(
        state_store,
        AttemptPromotionRequest {
            attempt_id: promotion.attempt_id.to_owned(),
            job_id: promotion.job_id.to_owned(),
            outputs: vec![AttemptOutputPromotion {
                source: staging.clone(),
                target_uri: promotion.target_uri,
                artifact_type: promotion.artifact_type.to_owned(),
                metadata: promotion.metadata,
                expected_sha256: None,
            }],
            selected_output_index: 0,
        },
    );
    let _ = fs::remove_file(&staging);
    cleanup_empty_parent_v1(staging.parent());
    result?.into_iter().next().ok_or_else(|| {
        Error::InvalidArtifact("creator attempt produced no promoted artifact".to_owned())
    })
}

fn write_staging_json_v1<T: Serialize>(
    artifact_store: &ArtifactStore,
    value: &T,
) -> Result<PathBuf> {
    let directory = artifact_store
        .data_root()
        .join("cache")
        .join("creator-orchestration");
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{}.json", Uuid::new_v4().simple()));
    let bytes = serde_json::to_vec(value)?;
    fs::write(&path, bytes)?;
    Ok(path)
}

fn cleanup_empty_parent_v1(parent: Option<&Path>) {
    if let Some(parent) = parent {
        let _ = fs::remove_dir(parent);
    }
}

fn read_json_artifact_v1<T: for<'de> Deserialize<'de>>(
    artifact_store: &ArtifactStore,
    artifact: &Artifact,
) -> Result<T> {
    if !artifact_store.verify_artifact(artifact)? {
        return Err(Error::ArtifactNotFound(artifact.artifact_id.clone()));
    }
    let path = artifact_store.resolve_artifact_path(artifact)?;
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn content_target_uri_v1(job_id: &str) -> Result<LogicalUri> {
    LogicalUri::parse(&format!(
        "project://content/{job_id}.creator-content.v1.json"
    ))
}

fn scene_target_uri_v1(job_id: &str) -> Result<LogicalUri> {
    LogicalUri::parse(&format!(
        "project://scenes/{job_id}.creator-scene-plan.v1.json"
    ))
}

fn is_sha256_hex_v1(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;
    use crate::{
        build_studio_review_center_v1, compile_creator_workflow_plan_v1,
        initial_studio_pack_catalog_v1, materialize_creator_workflow_plan_v1,
        StudioJobReviewSnapshotV1, StudioReviewActionV1, StudioReviewKindV1, Workspace,
    };

    #[derive(Default)]
    struct MockLlm {
        script_calls: Cell<usize>,
        scene_calls: Cell<usize>,
        script: RefCell<Option<String>>,
        fail_setup: Cell<bool>,
    }

    impl MockLlm {
        fn with_script(script: &str) -> Self {
            Self {
                script: RefCell::new(Some(script.to_owned())),
                ..Default::default()
            }
        }
    }

    impl CreatorLlmExecutorV1 for MockLlm {
        fn create_script_v1(&self, input: &CreatorInputV1) -> Result<String> {
            self.script_calls.set(self.script_calls.get() + 1);
            if self.fail_setup.get() {
                return Err(Error::MissingLlmGatewayCredential(
                    "LLMGATEWAY_API_KEY".to_owned(),
                ));
            }
            match input.kind {
                CreatorInputKindV1::Script => Ok(input.text.trim().to_owned()),
                CreatorInputKindV1::Topic => Ok(self
                    .script
                    .borrow()
                    .clone()
                    .unwrap_or_else(|| "A first visual beat. A second visual beat.".to_owned())),
            }
        }

        fn create_scene_intent_v1(
            &self,
            segment: &SegmentV1,
            scene_id: &str,
            options: &CreatorContentSceneOptionsV1,
        ) -> Result<SceneIntentV1> {
            self.scene_calls.set(self.scene_calls.get() + 1);
            if self.fail_setup.get() {
                return Err(Error::MissingLlmGatewayCredential(
                    "LLMGATEWAY_API_KEY".to_owned(),
                ));
            }
            Ok(SceneIntentV1 {
                schema: SCENE_INTENT_SCHEMA.to_owned(),
                schema_version: SCENE_INTENT_SCHEMA_VERSION,
                id: scene_id.to_owned(),
                segment_id: segment.id.clone(),
                narration: segment.text.clone(),
                purpose: "Translate the narration into a concrete visual beat.".to_owned(),
                scene_type: "conceptual".to_owned(),
                emotion_before: None,
                emotion_after: None,
                duration_hint: Some(8.0),
                visual_ideas: vec![
                    "hands rebuilding a weathered bridge".to_owned(),
                    "careful repair work in morning light".to_owned(),
                ],
                search_queries: vec![
                    "repairing wooden bridge morning".to_owned(),
                    "craftsperson careful restoration".to_owned(),
                    "hands rebuilding damaged structure".to_owned(),
                ],
                avoid: options.avoid.clone(),
                continuity: Default::default(),
                aspect_ratio: options.aspect_ratio.clone(),
            })
        }
    }

    fn fixture() -> (
        tempfile::TempDir,
        Workspace,
        StateStore,
        ArtifactStore,
        String,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let state = StateStore::open(workspace.sqlite_path()).unwrap();
        let pack = initial_studio_pack_catalog_v1()
            .unwrap()
            .resolve_v1("christian-cinematic")
            .unwrap();
        let project = state
            .create_project_with_studio_pack("Creator flow", Some(&pack.id))
            .unwrap();
        let plan = compile_creator_workflow_plan_v1(&project, &pack).unwrap();
        materialize_creator_workflow_plan_v1(&state, &plan).unwrap();
        let artifacts = ArtifactStore::new(workspace.data_root()).unwrap();
        let project_id = project.id.clone();
        (temp, workspace, state, artifacts, project_id)
    }

    #[test]
    fn creator_input_is_portable_and_provider_neutral() {
        let input = CreatorInputV1::topic("Why forgiveness does not automatically restore trust");
        let value = input.canonical_json_v1().unwrap();

        assert!(value.contains("TOPIC"));
        for forbidden in [
            "provider", "account", "endpoint", "api_key", "model_id", "/Users/", "/home/",
        ] {
            assert!(!value.contains(forbidden));
        }
    }

    #[test]
    fn script_segmentation_is_deterministic_and_preserves_order() {
        let first = segment_script_v1("First beat. Second beat? Final beat!").unwrap();
        let second = segment_script_v1("First beat. Second beat? Final beat!").unwrap();

        assert_eq!(first, second);
        assert_eq!(first.len(), 3);
        assert_eq!(first[0].id, "S001");
        assert_eq!(first[2].id, "S003");
        assert_eq!(first[1].text, "Second beat?");
    }

    #[test]
    fn topic_to_scene_plan_records_canonical_jobs_attempts_and_artifacts() {
        let (_temp, _workspace, mut state, artifacts, project_id) = fixture();
        let llm = MockLlm::with_script("First visual beat. Second visual beat.");
        let input = CreatorInputV1::topic("A practical lesson");
        let options = CreatorContentSceneOptionsV1 {
            avoid: vec!["generic praying hands".to_owned()],
            ..Default::default()
        };

        let outcome = run_creator_content_scene_v1(
            &mut state,
            &artifacts,
            &llm,
            &project_id,
            &input,
            &options,
        )
        .unwrap();

        assert!(!outcome.content_cache_hit);
        assert!(!outcome.scene_plan_cache_hit);
        assert_eq!(outcome.content.segments.len(), 2);
        assert_eq!(outcome.scene_plan.scenes.len(), 2);
        assert!(artifacts
            .verify_artifact(&outcome.content_artifact)
            .unwrap());
        assert!(artifacts
            .verify_artifact(&outcome.scene_plan_artifact)
            .unwrap());

        let jobs = state.list_project_jobs(&project_id).unwrap();
        assert_eq!(jobs.len(), 2);
        assert!(jobs.iter().all(|job| job.status == StepStatus::Succeeded));
        for job in jobs {
            let attempts = state.list_attempts(&job.job_id).unwrap();
            assert_eq!(attempts.len(), 1);
            assert_eq!(attempts[0].status, StepStatus::Succeeded);
        }

        let steps = state.list_project_steps(&project_id).unwrap();
        assert_eq!(
            find_project_step_v1(&steps, CREATOR_STEP_CONTENT_PREPARE_V1)
                .unwrap()
                .status,
            StepStatus::Succeeded
        );
        assert_eq!(
            find_project_step_v1(&steps, CREATOR_STEP_SCENE_PLAN_V1)
                .unwrap()
                .status,
            StepStatus::Succeeded
        );
    }

    #[test]
    fn identical_resume_hits_verified_project_cache_without_new_llm_calls() {
        let (_temp, _workspace, mut state, artifacts, project_id) = fixture();
        let llm = MockLlm::with_script("First beat. Second beat.");
        let input = CreatorInputV1::topic("Cache me");
        let options = CreatorContentSceneOptionsV1::default();

        run_creator_content_scene_v1(&mut state, &artifacts, &llm, &project_id, &input, &options)
            .unwrap();
        let calls = (llm.script_calls.get(), llm.scene_calls.get());
        let jobs_before = state.list_project_jobs(&project_id).unwrap().len();

        let second = run_creator_content_scene_v1(
            &mut state,
            &artifacts,
            &llm,
            &project_id,
            &input,
            &options,
        )
        .unwrap();

        assert!(second.content_cache_hit);
        assert!(second.scene_plan_cache_hit);
        assert_eq!((llm.script_calls.get(), llm.scene_calls.get()), calls);
        assert_eq!(
            state.list_project_jobs(&project_id).unwrap().len(),
            jobs_before
        );
    }

    #[test]
    fn changed_creator_input_invalidates_downstream_and_recomputes_content_and_scenes() {
        let (_temp, _workspace, mut state, artifacts, project_id) = fixture();
        let llm = MockLlm::with_script("First beat. Second beat.");
        let options = CreatorContentSceneOptionsV1::default();

        run_creator_content_scene_v1(
            &mut state,
            &artifacts,
            &llm,
            &project_id,
            &CreatorInputV1::topic("Version one"),
            &options,
        )
        .unwrap();
        *llm.script.borrow_mut() = Some("Changed beat. Another changed beat.".to_owned());

        let second = run_creator_content_scene_v1(
            &mut state,
            &artifacts,
            &llm,
            &project_id,
            &CreatorInputV1::topic("Version two"),
            &options,
        )
        .unwrap();

        assert!(!second.content_cache_hit);
        assert!(!second.scene_plan_cache_hit);
        assert_eq!(llm.script_calls.get(), 2);

        let steps = state.list_project_steps(&project_id).unwrap();
        assert_eq!(
            find_project_step_v1(&steps, CREATOR_STEP_CONTENT_PREPARE_V1)
                .unwrap()
                .status,
            StepStatus::Succeeded
        );
        assert_eq!(
            find_project_step_v1(&steps, CREATOR_STEP_SCENE_PLAN_V1)
                .unwrap()
                .status,
            StepStatus::Succeeded
        );
        assert_eq!(
            find_project_step_v1(&steps, "visual.prepare")
                .unwrap()
                .status,
            StepStatus::Ready
        );
        assert_eq!(
            find_project_step_v1(&steps, "voice.prepare")
                .unwrap()
                .status,
            StepStatus::Ready
        );
        assert_eq!(
            find_project_step_v1(&steps, "production.pack")
                .unwrap()
                .status,
            StepStatus::NotReady
        );
    }

    #[test]
    fn missing_llmgateway_setup_is_retryable_and_surfaces_setup_action() {
        let (_temp, _workspace, mut state, artifacts, project_id) = fixture();
        let llm = MockLlm {
            fail_setup: Cell::new(true),
            ..Default::default()
        };

        let error = run_creator_content_scene_v1(
            &mut state,
            &artifacts,
            &llm,
            &project_id,
            &CreatorInputV1::topic("Requires gateway"),
            &CreatorContentSceneOptionsV1::default(),
        )
        .unwrap_err();
        assert!(matches!(error, Error::MissingLlmGatewayCredential(_)));

        let project = state.get_project(&project_id).unwrap();
        let jobs = state.list_project_jobs(&project_id).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, StepStatus::Retryable);
        let attempts = state.list_attempts(&jobs[0].job_id).unwrap();
        assert_eq!(
            attempts[0].error_code.as_deref(),
            Some(LLMGATEWAY_SETUP_REQUIRED_ERROR_V1)
        );

        let review = build_studio_review_center_v1(&[(
            project,
            vec![StudioJobReviewSnapshotV1 {
                job: jobs[0].clone(),
                attempts,
            }],
            state.list_project_steps(&project_id).unwrap(),
            None,
        )]);
        let setup = review
            .items
            .iter()
            .find(|item| item.kind == StudioReviewKindV1::SetupRequirement)
            .expect("setup review item");
        assert_eq!(
            setup.action,
            Some(StudioReviewActionV1::ConfigureLlmGateway)
        );
    }
}
