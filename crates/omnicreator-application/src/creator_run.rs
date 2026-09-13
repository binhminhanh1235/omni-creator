use omnicreator_core::{
    assemble_creator_production_pack_v1, derive_creator_run_coordinator_v1,
    load_latest_creator_content_scene_v1, load_latest_creator_content_v1, Artifact, ArtifactStore,
    CreatorContentSceneOptionsV1, CreatorContentSceneOutcomeV1, CreatorContentV1, CreatorInputV1,
    CreatorProductionPackOptionsV1, CreatorRunCoordinatorV1, EffectiveStudioPackV1, Project,
    StateStore, StepStatus, WorkflowStep, Workspace, WorkspaceSession,
    CREATOR_STEP_CONTENT_PREPARE_V1, CREATOR_STEP_PRODUCTION_PACK_V1, CREATOR_STEP_SCENE_PLAN_V1,
    CREATOR_STEP_VISUAL_PREPARE_V1, CREATOR_STEP_VOICE_PREPARE_V1,
    CREATOR_WORKFLOW_UNIT_PROJECT_V1,
};
use serde::{Deserialize, Serialize};

use crate::{ControlAccessV1, ControlErrorCodeV1, ControlErrorV1, ControlResultV1};

pub const CREATOR_RUN_CONTROL_SCHEMA_V1: &str = "omnicreator.creator-run-control";
pub const CREATOR_RUN_CONTROL_VERSION_V1: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CreatorRunControlOperationV1 {
    StartOrResume,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StartOrResumeCreatorRequestV1 {
    pub project_id: String,
    pub input: Option<CreatorInputV1>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreatorRunControlOutcomeV1 {
    pub schema: String,
    pub version: u32,
    pub operation: CreatorRunControlOperationV1,
    pub project_id: String,
    pub read_only: bool,
    pub progressed: bool,
    pub coordinator: CreatorRunCoordinatorV1,
}

/// Machine-local execution bridge used by the transport-neutral creator run controller.
///
/// Implementations may own provider/plugin/compute handles, but they must execute through the
/// existing canonical core stage APIs. Cross-stage sequencing and Phase 17 AUTO ON/OFF policy
/// belong to `CreatorRunControlServiceV1`, not to the transport adapter.
pub trait CreatorRunRuntimeV1 {
    fn resolve_studio_pack_v1(
        &mut self,
        studio_pack_id: &str,
    ) -> ControlResultV1<EffectiveStudioPackV1>;

    fn run_content_v1(
        &mut self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        project_id: &str,
        input: &CreatorInputV1,
    ) -> ControlResultV1<()>;

    fn run_scene_v1(
        &mut self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        project_id: &str,
        content: &CreatorContentV1,
        content_artifact: &Artifact,
        options: &CreatorContentSceneOptionsV1,
    ) -> ControlResultV1<()>;

    fn run_visual_v1(
        &mut self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        project: &Project,
        studio_pack: &EffectiveStudioPackV1,
        creator: &CreatorContentSceneOutcomeV1,
    ) -> ControlResultV1<bool>;

    fn run_voice_v1(
        &mut self,
        state_store: &mut StateStore,
        artifact_store: &ArtifactStore,
        studio_pack: &EffectiveStudioPackV1,
        content: &CreatorContentV1,
    ) -> ControlResultV1<bool>;
}

pub struct CreatorRunControlServiceV1<'a> {
    _workspace: &'a Workspace,
    store: StateStore,
    artifacts: ArtifactStore,
    access: ControlAccessV1,
}

impl<'a> CreatorRunControlServiceV1<'a> {
    pub fn for_writer(session: &'a WorkspaceSession) -> ControlResultV1<Self> {
        let workspace = session.workspace();
        Ok(Self {
            _workspace: workspace,
            store: StateStore::open(session.sqlite_path()).map_err(ControlErrorV1::from)?,
            artifacts: ArtifactStore::new(workspace.data_root()).map_err(ControlErrorV1::from)?,
            access: ControlAccessV1::Writable,
        })
    }

    pub fn for_read_only(workspace: &'a Workspace) -> ControlResultV1<Self> {
        Ok(Self {
            _workspace: workspace,
            store: StateStore::open_read_only(workspace.sqlite_path())
                .map_err(ControlErrorV1::from)?,
            artifacts: ArtifactStore::new(workspace.data_root()).map_err(ControlErrorV1::from)?,
            access: ControlAccessV1::ReadOnly,
        })
    }

    pub fn start_or_resume_v1(
        &mut self,
        request: &StartOrResumeCreatorRequestV1,
        runtime: &mut impl CreatorRunRuntimeV1,
    ) -> ControlResultV1<CreatorRunControlOutcomeV1> {
        self.require_writable_v1()?;
        let project_id = non_empty_v1("project_id", &request.project_id)?;
        if let Some(input) = &request.input {
            input.validate_v1().map_err(ControlErrorV1::from)?;
        }

        let project = self
            .store
            .get_project(project_id)
            .map_err(ControlErrorV1::from)?;
        let studio_pack_id = project
            .studio_pack
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                ControlErrorV1::new(
                    ControlErrorCodeV1::Blocked,
                    "bind a Studio Pack before creator production can continue",
                )
            })?;
        let studio_pack = runtime.resolve_studio_pack_v1(studio_pack_id)?;
        studio_pack.validate_v1().map_err(ControlErrorV1::from)?;
        if studio_pack.id != studio_pack_id {
            return Err(ControlErrorV1::new(
                ControlErrorCodeV1::InvalidInput,
                "resolved Studio Pack does not match the Project binding",
            ));
        }

        let mut progressed = false;
        let mut content_state =
            load_latest_creator_content_v1(&self.store, &self.artifacts, project_id)
                .map_err(ControlErrorV1::from)?;
        let input_changed = match (&request.input, &content_state) {
            (Some(input), Some((content, _))) => content.source != *input,
            (Some(_), None) => true,
            (None, _) => false,
        };

        if content_state.is_none() || input_changed {
            if !self.automatic_execution_enabled_v1(project_id, CREATOR_STEP_CONTENT_PREPARE_V1)? {
                return self.outcome_v1(project_id, progressed);
            }
            let input = request.input.as_ref().ok_or_else(|| {
                ControlErrorV1::new(
                    ControlErrorCodeV1::Blocked,
                    "creator topic or script is required until Content has a verified canonical artifact",
                )
            })?;
            runtime.run_content_v1(&mut self.store, &self.artifacts, project_id, input)?;
            progressed = true;
            content_state =
                load_latest_creator_content_v1(&self.store, &self.artifacts, project_id)
                    .map_err(ControlErrorV1::from)?;
        }

        let (content, content_artifact) = content_state.ok_or_else(|| {
            ControlErrorV1::new(
                ControlErrorCodeV1::Blocked,
                "verified creator Content is required before creator production can continue",
            )
        })?;

        let mut creator =
            load_latest_creator_content_scene_v1(&self.store, &self.artifacts, project_id)
                .map_err(ControlErrorV1::from)?
                .filter(|value| value.content_artifact.artifact_id == content_artifact.artifact_id);
        if creator.is_none() {
            if !self.automatic_execution_enabled_v1(project_id, CREATOR_STEP_SCENE_PLAN_V1)? {
                return self.outcome_v1(project_id, progressed);
            }
            runtime.run_scene_v1(
                &mut self.store,
                &self.artifacts,
                project_id,
                &content,
                &content_artifact,
                &CreatorContentSceneOptionsV1::default(),
            )?;
            progressed = true;
            creator =
                load_latest_creator_content_scene_v1(&self.store, &self.artifacts, project_id)
                    .map_err(ControlErrorV1::from)?;
        }
        let creator = creator.ok_or_else(|| {
            ControlErrorV1::new(
                ControlErrorCodeV1::Blocked,
                "verified creator Scene Plan is required before visual orchestration can continue",
            )
        })?;

        let visual_step = self.require_step_v1(project_id, CREATOR_STEP_VISUAL_PREPARE_V1)?;
        if visual_step.status != StepStatus::Succeeded {
            if !self.automatic_execution_enabled_for_step_v1(&visual_step)? {
                return self.outcome_v1(project_id, progressed);
            }
            let complete = runtime.run_visual_v1(
                &mut self.store,
                &self.artifacts,
                &project,
                &studio_pack,
                &creator,
            )?;
            progressed = true;
            if !complete {
                return self.outcome_v1(project_id, progressed);
            }
        }

        let voice_step = self.require_step_v1(project_id, CREATOR_STEP_VOICE_PREPARE_V1)?;
        if voice_step.status != StepStatus::Succeeded {
            if !self.automatic_execution_enabled_for_step_v1(&voice_step)? {
                return self.outcome_v1(project_id, progressed);
            }
            let complete = runtime.run_voice_v1(
                &mut self.store,
                &self.artifacts,
                &studio_pack,
                &creator.content,
            )?;
            progressed = true;
            if !complete {
                return self.outcome_v1(project_id, progressed);
            }
        }

        let production_step = self.require_step_v1(project_id, CREATOR_STEP_PRODUCTION_PACK_V1)?;
        if production_step.status != StepStatus::Succeeded {
            if !self.automatic_execution_enabled_for_step_v1(&production_step)? {
                return self.outcome_v1(project_id, progressed);
            }
            assemble_creator_production_pack_v1(
                &mut self.store,
                &self.artifacts,
                project_id,
                &CreatorProductionPackOptionsV1::default(),
            )
            .map_err(ControlErrorV1::from)?;
            progressed = true;
        }

        self.outcome_v1(project_id, progressed)
    }

    fn require_writable_v1(&self) -> ControlResultV1<()> {
        if self.access.is_read_only() {
            Err(ControlErrorV1::read_only())
        } else {
            Ok(())
        }
    }

    fn require_step_v1(&self, project_id: &str, step_key: &str) -> ControlResultV1<WorkflowStep> {
        self.store
            .list_project_steps(project_id)
            .map_err(ControlErrorV1::from)?
            .into_iter()
            .find(|step| step.step == step_key && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1)
            .ok_or_else(|| {
                ControlErrorV1::new(
                    ControlErrorCodeV1::InvalidTransition,
                    format!("creator workflow step {step_key} is missing"),
                )
            })
    }

    fn automatic_execution_enabled_v1(
        &self,
        project_id: &str,
        step_key: &str,
    ) -> ControlResultV1<bool> {
        let step = self.require_step_v1(project_id, step_key)?;
        self.automatic_execution_enabled_for_step_v1(&step)
    }

    fn automatic_execution_enabled_for_step_v1(
        &self,
        step: &WorkflowStep,
    ) -> ControlResultV1<bool> {
        Ok(self
            .store
            .workflow_step_execution_policy_v1(&step.step_id)
            .map_err(ControlErrorV1::from)?
            .automatic_execution_enabled)
    }

    fn outcome_v1(
        &self,
        project_id: &str,
        progressed: bool,
    ) -> ControlResultV1<CreatorRunControlOutcomeV1> {
        let coordinator =
            derive_creator_run_coordinator_v1(&self.store, &self.artifacts, project_id)
                .map_err(ControlErrorV1::from)?;
        Ok(CreatorRunControlOutcomeV1 {
            schema: CREATOR_RUN_CONTROL_SCHEMA_V1.to_owned(),
            version: CREATOR_RUN_CONTROL_VERSION_V1,
            operation: CreatorRunControlOperationV1::StartOrResume,
            project_id: project_id.to_owned(),
            read_only: self.access.is_read_only(),
            progressed,
            coordinator,
        })
    }
}

fn non_empty_v1<'a>(label: &str, value: &'a str) -> ControlResultV1<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ControlErrorV1::new(
            ControlErrorCodeV1::InvalidInput,
            format!("{label} must not be empty"),
        ));
    }
    Ok(value)
}
