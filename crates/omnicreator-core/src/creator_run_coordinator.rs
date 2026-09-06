use serde::{Deserialize, Serialize};

use crate::{
    ArtifactStore, Error, Result, StateStore, StepStatus, CREATOR_STEP_CONTENT_PREPARE_V1,
    CREATOR_STEP_PRODUCTION_PACK_V1, CREATOR_STEP_SCENE_PLAN_V1, CREATOR_STEP_VISUAL_PREPARE_V1,
    CREATOR_STEP_VOICE_PREPARE_V1, CREATOR_TTS_STEP_V1, CREATOR_WORKFLOW_UNIT_PROJECT_V1,
};

pub const CREATOR_RUN_COORDINATOR_SCHEMA_V1: &str = "omnicreator.creator-run-coordinator";
pub const CREATOR_RUN_COORDINATOR_VERSION_V1: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CreatorRunStageV1 {
    ContentScene,
    Visual,
    VoiceCompute,
    ProductionPack,
    ReadyToEdit,
    Done,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CreatorRunActionV1 {
    StartOrResume,
    Review,
    RunCompute,
    WaitForCompute,
    AssembleProductionPack,
    Export,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreatorRunCoordinatorV1 {
    pub schema: String,
    pub version: u32,
    pub project_id: String,
    pub stage: CreatorRunStageV1,
    pub action: CreatorRunActionV1,
    pub blocking_step: Option<String>,
    pub message: String,
}

impl CreatorRunCoordinatorV1 {
    pub fn is_terminal_v1(&self) -> bool {
        matches!(
            self.action,
            CreatorRunActionV1::Export | CreatorRunActionV1::Complete
        )
    }
}

pub fn derive_creator_run_coordinator_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    project_id: &str,
) -> Result<CreatorRunCoordinatorV1> {
    let project = state_store.get_project(project_id)?;
    if project
        .studio_pack
        .as_deref()
        .map_or(true, str::is_empty)
    {
        return Ok(snapshot(
            project_id,
            CreatorRunStageV1::ContentScene,
            CreatorRunActionV1::Review,
            Some("studio_pack"),
            "Bind a Studio Pack before creator production can continue.",
        ));
    }

    let steps = state_store.list_project_steps(project_id)?;
    let step = |key: &str| {
        steps
            .iter()
            .find(|value| value.step == key && value.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1)
    };

    let content = step(CREATOR_STEP_CONTENT_PREPARE_V1).ok_or_else(|| {
        Error::InvalidContract("creator content workflow step is missing".to_owned())
    })?;
    let scenes = step(CREATOR_STEP_SCENE_PLAN_V1).ok_or_else(|| {
        Error::InvalidContract("creator scene workflow step is missing".to_owned())
    })?;
    let visual = step(CREATOR_STEP_VISUAL_PREPARE_V1).ok_or_else(|| {
        Error::InvalidContract("creator visual workflow step is missing".to_owned())
    })?;
    let voice = step(CREATOR_STEP_VOICE_PREPARE_V1).ok_or_else(|| {
        Error::InvalidContract("creator voice workflow step is missing".to_owned())
    })?;
    let production = step(CREATOR_STEP_PRODUCTION_PACK_V1).ok_or_else(|| {
        Error::InvalidContract("creator production-pack workflow step is missing".to_owned())
    })?;

    if let Some(blocked) = [content, scenes, visual, voice, production]
        .into_iter()
        .find(|value| matches!(value.status, StepStatus::Failed | StepStatus::Fatal))
    {
        return Ok(snapshot(
            project_id,
            stage_for_step_v1(&blocked.step),
            CreatorRunActionV1::Review,
            Some(&blocked.step),
            &format!(
                "{} needs review before creator production can continue.",
                blocked.step
            ),
        ));
    }

    if content.status != StepStatus::Succeeded || scenes.status != StepStatus::Succeeded {
        return Ok(snapshot(
            project_id,
            CreatorRunStageV1::ContentScene,
            CreatorRunActionV1::StartOrResume,
            Some(if content.status != StepStatus::Succeeded {
                CREATOR_STEP_CONTENT_PREPARE_V1
            } else {
                CREATOR_STEP_SCENE_PLAN_V1
            }),
            "Start or resume canonical content and SceneIntent preparation.",
        ));
    }

    if visual.status != StepStatus::Succeeded {
        let retryable_visual = state_store
            .list_project_jobs(project_id)?
            .into_iter()
            .any(|job| {
                job.step == CREATOR_STEP_VISUAL_PREPARE_V1
                    && matches!(job.status, StepStatus::Retryable | StepStatus::Failed)
            });
        return Ok(snapshot(
            project_id,
            CreatorRunStageV1::Visual,
            if retryable_visual {
                CreatorRunActionV1::Review
            } else {
                CreatorRunActionV1::StartOrResume
            },
            Some(CREATOR_STEP_VISUAL_PREPARE_V1),
            if retryable_visual {
                "A visual job needs review or retry before the visual stage can complete."
            } else {
                "Continue visual routing, review and selected-asset execution."
            },
        ));
    }

    if voice.status != StepStatus::Succeeded {
        let tts_jobs = state_store
            .list_project_jobs(project_id)?
            .into_iter()
            .filter(|job| job.step == CREATOR_TTS_STEP_V1)
            .collect::<Vec<_>>();
        if tts_jobs.iter().any(|job| job.status == StepStatus::Running) {
            return Ok(snapshot(
                project_id,
                CreatorRunStageV1::VoiceCompute,
                CreatorRunActionV1::WaitForCompute,
                Some(CREATOR_STEP_VOICE_PREPARE_V1),
                "Voice compute is running. Resume will reconcile canonical remote state.",
            ));
        }
        if tts_jobs.iter().any(|job| {
            matches!(
                job.status,
                StepStatus::Retryable | StepStatus::Failed | StepStatus::Fatal
            )
        }) {
            return Ok(snapshot(
                project_id,
                CreatorRunStageV1::VoiceCompute,
                CreatorRunActionV1::Review,
                Some(CREATOR_STEP_VOICE_PREPARE_V1),
                "A voice job needs review or retry before compute can continue.",
            ));
        }
        if tts_jobs
            .iter()
            .any(|job| matches!(job.status, StepStatus::Ready | StepStatus::Queued))
        {
            return Ok(snapshot(
                project_id,
                CreatorRunStageV1::VoiceCompute,
                CreatorRunActionV1::RunCompute,
                Some(CREATOR_STEP_VOICE_PREPARE_V1),
                "Voice jobs are ready for the existing ComputeProvider/Burst boundary.",
            ));
        }
        return Ok(snapshot(
            project_id,
            CreatorRunStageV1::VoiceCompute,
            CreatorRunActionV1::StartOrResume,
            Some(CREATOR_STEP_VOICE_PREPARE_V1),
            "Materialize or resume canonical voice jobs from creator content.",
        ));
    }

    if production.status != StepStatus::Succeeded {
        return Ok(snapshot(
            project_id,
            CreatorRunStageV1::ProductionPack,
            CreatorRunActionV1::AssembleProductionPack,
            Some(CREATOR_STEP_PRODUCTION_PACK_V1),
            "Visual and voice artifacts are complete. Assemble the canonical ProductionPack.",
        ));
    }

    if crate::load_latest_creator_production_pack_v1(state_store, artifact_store, project_id)?
        .is_none()
    {
        return Ok(snapshot(
            project_id,
            CreatorRunStageV1::ProductionPack,
            CreatorRunActionV1::AssembleProductionPack,
            Some(CREATOR_STEP_PRODUCTION_PACK_V1),
            "ProductionPack state is marked complete but its verified artifact is unavailable.",
        ));
    }

    let exported = state_store
        .list_project_jobs(project_id)?
        .into_iter()
        .any(|job| job.step == "export.production-pack" && job.status == StepStatus::Succeeded);
    if exported {
        Ok(snapshot(
            project_id,
            CreatorRunStageV1::Done,
            CreatorRunActionV1::Complete,
            None,
            "Creator production and Resolve export are complete.",
        ))
    } else {
        Ok(snapshot(
            project_id,
            CreatorRunStageV1::ReadyToEdit,
            CreatorRunActionV1::Export,
            None,
            "ProductionPack is ready for review and Resolve export.",
        ))
    }
}

fn stage_for_step_v1(step: &str) -> CreatorRunStageV1 {
    match step {
        CREATOR_STEP_CONTENT_PREPARE_V1 | CREATOR_STEP_SCENE_PLAN_V1 => {
            CreatorRunStageV1::ContentScene
        }
        CREATOR_STEP_VISUAL_PREPARE_V1 => CreatorRunStageV1::Visual,
        CREATOR_STEP_VOICE_PREPARE_V1 => CreatorRunStageV1::VoiceCompute,
        CREATOR_STEP_PRODUCTION_PACK_V1 => CreatorRunStageV1::ProductionPack,
        _ => CreatorRunStageV1::ContentScene,
    }
}

fn snapshot(
    project_id: &str,
    stage: CreatorRunStageV1,
    action: CreatorRunActionV1,
    blocking_step: Option<&str>,
    message: &str,
) -> CreatorRunCoordinatorV1 {
    CreatorRunCoordinatorV1 {
        schema: CREATOR_RUN_COORDINATOR_SCHEMA_V1.to_owned(),
        version: CREATOR_RUN_COORDINATOR_VERSION_V1,
        project_id: project_id.to_owned(),
        stage,
        action,
        blocking_step: blocking_step.map(str::to_owned),
        message: message.to_owned(),
    }
}
