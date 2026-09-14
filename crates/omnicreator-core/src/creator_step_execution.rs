use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{de::DeserializeOwned, Serialize};
use uuid::Uuid;

use crate::{
    artifact_store::{AttemptOutputPromotion, AttemptPromotionRequest},
    deterministic_input_hash, segment_creator_script_v1, Artifact, ArtifactStore,
    CreatorContentSceneOptionsV1, CreatorContentV1, CreatorInputV1, CreatorLlmExecutorV1,
    CreatorScenePlanV1, Error, LogicalUri, Result, StateStore, StepStatus, WorkflowStep,
    CREATOR_CONTENT_ARTIFACT_TYPE_V1, CREATOR_CONTENT_SCHEMA_V1, CREATOR_CONTENT_VERSION_V1,
    CREATOR_SCENE_PLAN_ARTIFACT_TYPE_V1, CREATOR_SCENE_PLAN_SCHEMA_V1,
    CREATOR_SCENE_PLAN_VERSION_V1, CREATOR_STEP_CONTENT_PREPARE_V1, CREATOR_STEP_SCENE_PLAN_V1,
    CREATOR_WORKFLOW_UNIT_PROJECT_V1, LLMGATEWAY_SETUP_REQUIRED_ERROR_V1, SCENE_INTENT_SCHEMA,
    SCENE_INTENT_SCHEMA_VERSION,
};

const CREATOR_CONTENT_WORKER_PHASE17_V1: &str = "creator-content-v1";
const CREATOR_SCENE_WORKER_PHASE17_V1: &str = "llmgateway-scene-intelligence-v1";

pub fn run_creator_content_stage_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    llm: &impl CreatorLlmExecutorV1,
    project_id: &str,
    input: &CreatorInputV1,
) -> Result<(CreatorContentV1, Artifact, bool)> {
    input.validate_v1()?;
    let project = require_creator_project_v1(state_store, project_id)?;
    let step = require_creator_step_v1(state_store, project_id, CREATOR_STEP_CONTENT_PREPARE_V1)?;
    let input_json = input.canonical_json_v1()?;
    let input_hash = deterministic_input_hash(&[
        b"creator-content-input-v1",
        project.id.as_bytes(),
        project.script_version.to_string().as_bytes(),
        input_json.as_bytes(),
    ]);

    if let Some(artifact) = find_verified_cache_v1(
        state_store,
        artifact_store,
        project_id,
        CREATOR_STEP_CONTENT_PREPARE_V1,
        &input_hash,
        CREATOR_CONTENT_ARTIFACT_TYPE_V1,
    )? {
        let content: CreatorContentV1 = read_json_artifact_v1(artifact_store, &artifact)?;
        content.validate_v1()?;
        mark_step_succeeded_v1(state_store, &step.step_id)?;
        state_store.refresh_ready_steps(project_id)?;
        return Ok((content, artifact, true));
    }

    prepare_step_for_new_input_v1(state_store, &step)?;
    let job = get_or_create_job_v1(
        state_store,
        project_id,
        CREATOR_STEP_CONTENT_PREPARE_V1,
        CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        &input_hash,
    )?;
    state_store.set_step_status(&step.step_id, StepStatus::Running)?;
    let attempt =
        state_store.start_attempt(&job.job_id, Some(CREATOR_CONTENT_WORKER_PHASE17_V1))?;

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
            project_id: project_id.to_owned(),
            source: input.clone(),
            script: script.trim().to_owned(),
            segments: segment_creator_script_v1(script.trim())?,
        };
        content.validate_v1()?;
        let target_uri = LogicalUri::parse(&format!(
            "project://content/{}.creator-content.v1.json",
            job.job_id
        ))?;
        let artifact = promote_json_for_attempt_v1(
            artifact_store,
            state_store,
            &attempt.attempt_id,
            &job.job_id,
            &content,
            target_uri,
            CREATOR_CONTENT_ARTIFACT_TYPE_V1,
            serde_json::json!({
                "schema": CREATOR_CONTENT_SCHEMA_V1,
                "schema_version": CREATOR_CONTENT_VERSION_V1,
                "stage": CREATOR_STEP_CONTENT_PREPARE_V1,
                "input_kind": input.kind,
            }),
        )?;
        Ok((content, artifact))
    })();

    match result {
        Ok((content, artifact)) => {
            state_store.set_step_status(&step.step_id, StepStatus::Succeeded)?;
            state_store.refresh_ready_steps(project_id)?;
            Ok((content, artifact, false))
        }
        Err(error) => {
            finish_failure_v1(state_store, &step.step_id, &attempt.attempt_id, &error)?;
            Err(error)
        }
    }
}

pub fn run_creator_scene_stage_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    llm: &impl CreatorLlmExecutorV1,
    project_id: &str,
    content: &CreatorContentV1,
    content_artifact: &Artifact,
    options: &CreatorContentSceneOptionsV1,
) -> Result<(CreatorScenePlanV1, Artifact, bool)> {
    require_creator_project_v1(state_store, project_id)?;
    content.validate_v1()?;
    options.validate_v1()?;
    if content.project_id != project_id
        || content_artifact.project_id.as_deref() != Some(project_id)
    {
        return Err(Error::InvalidContract(
            "creator content identity must match the requested project".to_owned(),
        ));
    }
    if content_artifact.artifact_type != CREATOR_CONTENT_ARTIFACT_TYPE_V1
        || !artifact_store.verify_artifact(content_artifact)?
    {
        return Err(Error::InvalidArtifact(
            "scene execution requires a verified canonical creator content artifact".to_owned(),
        ));
    }

    state_store.refresh_ready_steps(project_id)?;
    let step = require_creator_step_v1(state_store, project_id, CREATOR_STEP_SCENE_PLAN_V1)?;
    if step.status == StepStatus::NotReady {
        return Err(Error::InvalidTransition(
            "scene.plan is still waiting on content.prepare".to_owned(),
        ));
    }
    let options_json = serde_json::to_string(options)?;
    let input_hash = deterministic_input_hash(&[
        b"creator-scene-input-v1",
        content_artifact.sha256.as_bytes(),
        options_json.as_bytes(),
    ]);

    if let Some(artifact) = find_verified_cache_v1(
        state_store,
        artifact_store,
        project_id,
        CREATOR_STEP_SCENE_PLAN_V1,
        &input_hash,
        CREATOR_SCENE_PLAN_ARTIFACT_TYPE_V1,
    )? {
        let scene_plan: CreatorScenePlanV1 = read_json_artifact_v1(artifact_store, &artifact)?;
        scene_plan.validate_v1(content)?;
        if scene_plan.content_sha256 != content_artifact.sha256 {
            return Err(Error::InvalidArtifact(
                "cached creator scene plan does not reference the selected content artifact"
                    .to_owned(),
            ));
        }
        mark_step_succeeded_v1(state_store, &step.step_id)?;
        state_store.refresh_ready_steps(project_id)?;
        return Ok((scene_plan, artifact, true));
    }

    prepare_step_for_new_input_v1(state_store, &step)?;
    let job = get_or_create_job_v1(
        state_store,
        project_id,
        CREATOR_STEP_SCENE_PLAN_V1,
        CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        &input_hash,
    )?;
    state_store.set_step_status(&step.step_id, StepStatus::Running)?;
    let attempt = state_store.start_attempt(&job.job_id, Some(CREATOR_SCENE_WORKER_PHASE17_V1))?;

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
            project_id: project_id.to_owned(),
            content_sha256: content_artifact.sha256.clone(),
            scenes,
        };
        scene_plan.validate_v1(content)?;
        let target_uri = LogicalUri::parse(&format!(
            "project://scenes/{}.creator-scene-plan.v1.json",
            job.job_id
        ))?;
        let artifact = promote_json_for_attempt_v1(
            artifact_store,
            state_store,
            &attempt.attempt_id,
            &job.job_id,
            &scene_plan,
            target_uri,
            CREATOR_SCENE_PLAN_ARTIFACT_TYPE_V1,
            serde_json::json!({
                "schema": CREATOR_SCENE_PLAN_SCHEMA_V1,
                "schema_version": CREATOR_SCENE_PLAN_VERSION_V1,
                "stage": CREATOR_STEP_SCENE_PLAN_V1,
                "scene_count": scene_plan.scenes.len(),
                "content_sha256": content_artifact.sha256,
            }),
        )?;
        Ok((scene_plan, artifact))
    })();

    match result {
        Ok((scene_plan, artifact)) => {
            state_store.set_step_status(&step.step_id, StepStatus::Succeeded)?;
            state_store.refresh_ready_steps(project_id)?;
            Ok((scene_plan, artifact, false))
        }
        Err(error) => {
            finish_failure_v1(state_store, &step.step_id, &attempt.attempt_id, &error)?;
            Err(error)
        }
    }
}

fn require_creator_project_v1(
    state_store: &StateStore,
    project_id: &str,
) -> Result<crate::Project> {
    let project = state_store.get_project(project_id)?;
    if project.studio_pack.as_deref().is_none_or(str::is_empty) {
        return Err(Error::InvalidContract(
            "creator step execution requires a Project bound to a Studio Pack".to_owned(),
        ));
    }
    Ok(project)
}

fn require_creator_step_v1(
    state_store: &StateStore,
    project_id: &str,
    step_key: &str,
) -> Result<WorkflowStep> {
    state_store
        .list_project_steps(project_id)?
        .into_iter()
        .find(|step| step.step == step_key && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1)
        .ok_or_else(|| {
            Error::InvalidContract(format!(
                "creator workflow step {step_key}/{} is missing; materialize Phase 15 P0 first",
                CREATOR_WORKFLOW_UNIT_PROJECT_V1
            ))
        })
}

fn find_verified_cache_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
    step_key: &str,
    input_hash: &str,
    artifact_type: &str,
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
            || artifact.artifact_type != artifact_type
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
            "creator job {} is FATAL for unchanged input; change input or resolve the blocker",
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
        "workflow step {step_id} cannot reuse a verified cache from {}",
        current.status.as_str()
    )))
}

fn finish_failure_v1(
    state_store: &mut StateStore,
    step_id: &str,
    attempt_id: &str,
    error: &Error,
) -> Result<()> {
    let attempt = state_store.finish_attempt_failure(attempt_id, creator_error_code_v1(error))?;
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
        Error::InvalidStructuredOutput { .. } | Error::InvalidLlmGatewayResponse(_) => {
            "INVALID_LLM_OUTPUT"
        }
        _ => "LOCAL_RUNTIME_CONTEXT_ERROR",
    }
}

#[allow(clippy::too_many_arguments)]
fn promote_json_for_attempt_v1<T: Serialize>(
    artifact_store: &ArtifactStore,
    state_store: &mut StateStore,
    attempt_id: &str,
    job_id: &str,
    value: &T,
    target_uri: LogicalUri,
    artifact_type: &str,
    metadata: serde_json::Value,
) -> Result<Artifact> {
    let staging = write_staging_json_v1(artifact_store, value)?;
    let result = artifact_store.promote_attempt_outputs(
        state_store,
        AttemptPromotionRequest {
            attempt_id: attempt_id.to_owned(),
            job_id: job_id.to_owned(),
            outputs: vec![AttemptOutputPromotion {
                source: staging.clone(),
                target_uri,
                artifact_type: artifact_type.to_owned(),
                metadata,
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
        .join("creator-step-execution");
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{}.json", Uuid::new_v4().simple()));
    fs::write(&path, serde_json::to_vec(value)?)?;
    Ok(path)
}

fn cleanup_empty_parent_v1(parent: Option<&Path>) {
    if let Some(parent) = parent {
        let _ = fs::remove_dir(parent);
    }
}

fn read_json_artifact_v1<T: DeserializeOwned>(
    artifact_store: &ArtifactStore,
    artifact: &Artifact,
) -> Result<T> {
    if !artifact_store.verify_artifact(artifact)? {
        return Err(Error::ArtifactNotFound(artifact.artifact_id.clone()));
    }
    let path = artifact_store.resolve_artifact_path(artifact)?;
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
