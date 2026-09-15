use omnicreator_core::{
    ExternalGeneratedVisualRequestV1, ExternalVoiceRequestV1, Job, StepStatus, WorkflowStep,
    CREATOR_STEP_CONTENT_PREPARE_V1, CREATOR_STEP_PRODUCTION_PACK_V1, CREATOR_STEP_SCENE_PLAN_V1,
    CREATOR_STEP_VISUAL_PREPARE_V1, CREATOR_STEP_VOICE_PREPARE_V1, CREATOR_TTS_STEP_V1,
    CREATOR_WORKFLOW_UNIT_PROJECT_V1,
};
use serde::{Deserialize, Serialize};

use crate::{
    ApplicationControlService, ControlErrorCodeV1, ControlErrorV1, ControlResultV1,
    ProjectControlSnapshotV1, ProjectIdRequestV1,
};

pub const AGENT_WORK_GRAPH_SCHEMA_V1: &str = "omnicreator.agent-work-graph";
pub const AGENT_WORK_GRAPH_VERSION_V1: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkKindV1 {
    Content,
    ScenePlan,
    Visual,
    Voice,
    ProductionPack,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgentWorkStateV1 {
    Blocked,
    Ready,
    Running,
    NeedsReview,
    Satisfied,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkActionV1 {
    ProvideContent,
    ProvideScenePlan,
    ProvideVisual,
    ProvideVoice,
    AssembleProductionPack,
    Review,
    Wait,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentExternalWorkDescriptorV1 {
    Visual {
        request: ExternalGeneratedVisualRequestV1,
    },
    Voice {
        request: ExternalVoiceRequestV1,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentWorkItemV1 {
    pub work_id: String,
    pub kind: AgentWorkKindV1,
    pub canonical_step: String,
    pub canonical_unit: String,
    pub state: AgentWorkStateV1,
    pub dependencies: Vec<String>,
    pub suggested_action: Option<AgentWorkActionV1>,
    pub input_sha256: Option<String>,
    pub selected_artifact_ids: Vec<String>,
    pub external: Option<AgentExternalWorkDescriptorV1>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentWorkGraphV1 {
    pub schema: String,
    pub version: u32,
    pub project_id: String,
    pub read_only: bool,
    pub items: Vec<AgentWorkItemV1>,
    pub ready_work_ids: Vec<String>,
    pub running_work_ids: Vec<String>,
    pub needs_review_work_ids: Vec<String>,
    pub satisfied_work_ids: Vec<String>,
}

impl<'a> ApplicationControlService<'a> {
    /// Derives an agent-facing work graph from canonical creator state.
    ///
    /// This is a read-only projection. It deliberately does not create claims,
    /// queues or worker records, so an agent harness cannot become a second
    /// source of workflow truth simply by inspecting the graph.
    pub fn agent_work_graph_v1(
        &self,
        request: &ProjectIdRequestV1,
    ) -> ControlResultV1<AgentWorkGraphV1> {
        derive_agent_work_graph_v1(self, request)
    }
}

pub fn derive_agent_work_graph_v1(
    service: &ApplicationControlService<'_>,
    request: &ProjectIdRequestV1,
) -> ControlResultV1<AgentWorkGraphV1> {
    let snapshot = service.project_status_v1(request)?.data;
    let content_step = project_step_v1(&snapshot, CREATOR_STEP_CONTENT_PREPARE_V1)?;
    let scene_step = project_step_v1(&snapshot, CREATOR_STEP_SCENE_PLAN_V1)?;
    let visual_step = project_step_v1(&snapshot, CREATOR_STEP_VISUAL_PREPARE_V1)?;
    let voice_step = project_step_v1(&snapshot, CREATOR_STEP_VOICE_PREPARE_V1)?;
    let production_step = project_step_v1(&snapshot, CREATOR_STEP_PRODUCTION_PACK_V1)?;

    let mut items = Vec::new();

    let content_state = state_from_step_v1(content_step, true);
    items.push(AgentWorkItemV1 {
        work_id: work_id_v1(AgentWorkKindV1::Content, CREATOR_WORKFLOW_UNIT_PROJECT_V1),
        kind: AgentWorkKindV1::Content,
        canonical_step: CREATOR_STEP_CONTENT_PREPARE_V1.to_owned(),
        canonical_unit: CREATOR_WORKFLOW_UNIT_PROJECT_V1.to_owned(),
        state: content_state,
        dependencies: Vec::new(),
        suggested_action: suggested_action_v1(AgentWorkKindV1::Content, content_state),
        input_sha256: content_step.input_hash.clone(),
        selected_artifact_ids: selected_artifact_ids_v1(
            &snapshot.jobs,
            CREATOR_STEP_CONTENT_PREPARE_V1,
            CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        ),
        external: None,
        message: message_v1(AgentWorkKindV1::Content, content_state),
    });

    let content_satisfied = content_state == AgentWorkStateV1::Satisfied;
    let scene_state = state_from_step_v1(scene_step, content_satisfied);
    items.push(AgentWorkItemV1 {
        work_id: work_id_v1(AgentWorkKindV1::ScenePlan, CREATOR_WORKFLOW_UNIT_PROJECT_V1),
        kind: AgentWorkKindV1::ScenePlan,
        canonical_step: CREATOR_STEP_SCENE_PLAN_V1.to_owned(),
        canonical_unit: CREATOR_WORKFLOW_UNIT_PROJECT_V1.to_owned(),
        state: scene_state,
        dependencies: vec![work_id_v1(
            AgentWorkKindV1::Content,
            CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        )],
        suggested_action: suggested_action_v1(AgentWorkKindV1::ScenePlan, scene_state),
        input_sha256: scene_step.input_hash.clone(),
        selected_artifact_ids: selected_artifact_ids_v1(
            &snapshot.jobs,
            CREATOR_STEP_SCENE_PLAN_V1,
            CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        ),
        external: None,
        message: message_v1(AgentWorkKindV1::ScenePlan, scene_state),
    });

    if content_satisfied {
        for state in service.voice_states_v1(request)? {
            let external_request = service
                .prepare_external_voice_v1(&request.project_id, &state.segment_id)?
                .data;
            let work_state = if state.verified {
                AgentWorkStateV1::Satisfied
            } else {
                state_from_jobs_v1(&snapshot.jobs, CREATOR_TTS_STEP_V1, &state.segment_id, true)
            };
            let mut selected_artifact_ids = Vec::new();
            if let Some(audio) = state.audio.as_ref() {
                selected_artifact_ids.push(audio.artifact_id.clone());
            }
            if let Some(timing) = state.timing_artifact.as_ref() {
                selected_artifact_ids.push(timing.artifact_id.clone());
            }
            items.push(AgentWorkItemV1 {
                work_id: work_id_v1(AgentWorkKindV1::Voice, &state.segment_id),
                kind: AgentWorkKindV1::Voice,
                canonical_step: CREATOR_TTS_STEP_V1.to_owned(),
                canonical_unit: state.segment_id.clone(),
                state: work_state,
                dependencies: vec![work_id_v1(
                    AgentWorkKindV1::Content,
                    CREATOR_WORKFLOW_UNIT_PROJECT_V1,
                )],
                suggested_action: suggested_action_v1(AgentWorkKindV1::Voice, work_state),
                input_sha256: Some(external_request.request_sha256.clone()),
                selected_artifact_ids,
                external: if work_state == AgentWorkStateV1::Satisfied {
                    None
                } else {
                    Some(AgentExternalWorkDescriptorV1::Voice {
                        request: external_request,
                    })
                },
                message: message_v1(AgentWorkKindV1::Voice, work_state),
            });
        }
    }

    let scene_satisfied = scene_state == AgentWorkStateV1::Satisfied;
    if scene_satisfied {
        for state in service.visual_states_v1(request)? {
            let external_request = service
                .prepare_external_visual_v1(&request.project_id, &state.scene_id)?
                .data;
            let work_state = if state.verified {
                AgentWorkStateV1::Satisfied
            } else {
                state_from_jobs_v1(
                    &snapshot.jobs,
                    CREATOR_STEP_VISUAL_PREPARE_V1,
                    &state.scene_id,
                    true,
                )
            };
            let selected_artifact_ids = state
                .selected_artifact
                .as_ref()
                .map(|artifact| vec![artifact.artifact_id.clone()])
                .unwrap_or_default();
            items.push(AgentWorkItemV1 {
                work_id: work_id_v1(AgentWorkKindV1::Visual, &state.scene_id),
                kind: AgentWorkKindV1::Visual,
                canonical_step: CREATOR_STEP_VISUAL_PREPARE_V1.to_owned(),
                canonical_unit: state.scene_id.clone(),
                state: work_state,
                dependencies: vec![work_id_v1(
                    AgentWorkKindV1::ScenePlan,
                    CREATOR_WORKFLOW_UNIT_PROJECT_V1,
                )],
                suggested_action: suggested_action_v1(AgentWorkKindV1::Visual, work_state),
                input_sha256: Some(external_request.request_sha256.clone()),
                selected_artifact_ids,
                external: if work_state == AgentWorkStateV1::Satisfied {
                    None
                } else {
                    Some(AgentExternalWorkDescriptorV1::Visual {
                        request: external_request,
                    })
                },
                message: message_v1(AgentWorkKindV1::Visual, work_state),
            });
        }
    }

    let visual_unit_ids = items
        .iter()
        .filter(|item| item.kind == AgentWorkKindV1::Visual)
        .map(|item| item.work_id.clone())
        .collect::<Vec<_>>();
    let voice_unit_ids = items
        .iter()
        .filter(|item| item.kind == AgentWorkKindV1::Voice)
        .map(|item| item.work_id.clone())
        .collect::<Vec<_>>();
    let fan_in_satisfied = visual_step.status == StepStatus::Succeeded
        && voice_step.status == StepStatus::Succeeded
        && !visual_unit_ids.is_empty()
        && !voice_unit_ids.is_empty();
    let production_state = state_from_step_v1(production_step, fan_in_satisfied);
    let mut production_dependencies = visual_unit_ids;
    production_dependencies.extend(voice_unit_ids);
    if production_dependencies.is_empty() {
        production_dependencies.push(work_id_v1(
            AgentWorkKindV1::ScenePlan,
            CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        ));
    }
    items.push(AgentWorkItemV1 {
        work_id: work_id_v1(
            AgentWorkKindV1::ProductionPack,
            CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        ),
        kind: AgentWorkKindV1::ProductionPack,
        canonical_step: CREATOR_STEP_PRODUCTION_PACK_V1.to_owned(),
        canonical_unit: CREATOR_WORKFLOW_UNIT_PROJECT_V1.to_owned(),
        state: production_state,
        dependencies: production_dependencies,
        suggested_action: suggested_action_v1(AgentWorkKindV1::ProductionPack, production_state),
        input_sha256: production_step.input_hash.clone(),
        selected_artifact_ids: selected_artifact_ids_v1(
            &snapshot.jobs,
            CREATOR_STEP_PRODUCTION_PACK_V1,
            CREATOR_WORKFLOW_UNIT_PROJECT_V1,
        ),
        external: None,
        message: message_v1(AgentWorkKindV1::ProductionPack, production_state),
    });

    let ready_work_ids = work_ids_in_state_v1(&items, AgentWorkStateV1::Ready);
    let running_work_ids = work_ids_in_state_v1(&items, AgentWorkStateV1::Running);
    let needs_review_work_ids = work_ids_in_state_v1(&items, AgentWorkStateV1::NeedsReview);
    let satisfied_work_ids = work_ids_in_state_v1(&items, AgentWorkStateV1::Satisfied);

    Ok(AgentWorkGraphV1 {
        schema: AGENT_WORK_GRAPH_SCHEMA_V1.to_owned(),
        version: AGENT_WORK_GRAPH_VERSION_V1,
        project_id: request.project_id.clone(),
        read_only: service.access_v1().is_read_only(),
        items,
        ready_work_ids,
        running_work_ids,
        needs_review_work_ids,
        satisfied_work_ids,
    })
}

fn project_step_v1<'a>(
    snapshot: &'a ProjectControlSnapshotV1,
    step_key: &str,
) -> ControlResultV1<&'a WorkflowStep> {
    snapshot
        .steps
        .iter()
        .find(|step| step.step == step_key && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1)
        .ok_or_else(|| {
            ControlErrorV1::new(
                ControlErrorCodeV1::InvalidInput,
                format!("creator workflow step is missing: {step_key}"),
            )
        })
}

fn state_from_step_v1(step: &WorkflowStep, dependency_satisfied: bool) -> AgentWorkStateV1 {
    if step.status == StepStatus::Succeeded {
        return AgentWorkStateV1::Satisfied;
    }
    if !dependency_satisfied {
        return AgentWorkStateV1::Blocked;
    }
    match step.status {
        StepStatus::Ready => AgentWorkStateV1::Ready,
        StepStatus::Queued | StepStatus::Running => AgentWorkStateV1::Running,
        StepStatus::Failed
        | StepStatus::Retryable
        | StepStatus::Fatal
        | StepStatus::Stale
        | StepStatus::Skipped
        | StepStatus::Cancelled => AgentWorkStateV1::NeedsReview,
        StepStatus::NotReady => AgentWorkStateV1::Blocked,
        StepStatus::Succeeded => AgentWorkStateV1::Satisfied,
    }
}

fn state_from_jobs_v1(
    jobs: &[Job],
    step: &str,
    unit: &str,
    dependency_satisfied: bool,
) -> AgentWorkStateV1 {
    if !dependency_satisfied {
        return AgentWorkStateV1::Blocked;
    }
    let matching = jobs
        .iter()
        .filter(|job| job.step == step && job.unit == unit)
        .collect::<Vec<_>>();
    if matching
        .iter()
        .any(|job| matches!(job.status, StepStatus::Queued | StepStatus::Running))
    {
        return AgentWorkStateV1::Running;
    }
    if matching.iter().any(|job| {
        matches!(
            job.status,
            StepStatus::Succeeded
                | StepStatus::Failed
                | StepStatus::Retryable
                | StepStatus::Fatal
                | StepStatus::Stale
                | StepStatus::Skipped
                | StepStatus::Cancelled
        )
    }) {
        return AgentWorkStateV1::NeedsReview;
    }
    AgentWorkStateV1::Ready
}

fn selected_artifact_ids_v1(jobs: &[Job], step: &str, unit: &str) -> Vec<String> {
    let mut ids = jobs
        .iter()
        .filter(|job| job.step == step && job.unit == unit)
        .filter_map(|job| job.selected_artifact.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    ids
}

fn work_id_v1(kind: AgentWorkKindV1, unit: &str) -> String {
    let kind = match kind {
        AgentWorkKindV1::Content => "content",
        AgentWorkKindV1::ScenePlan => "scene_plan",
        AgentWorkKindV1::Visual => "visual",
        AgentWorkKindV1::Voice => "voice",
        AgentWorkKindV1::ProductionPack => "production_pack",
    };
    format!("{kind}:{unit}")
}

fn work_ids_in_state_v1(items: &[AgentWorkItemV1], state: AgentWorkStateV1) -> Vec<String> {
    items
        .iter()
        .filter(|item| item.state == state)
        .map(|item| item.work_id.clone())
        .collect()
}

fn suggested_action_v1(
    kind: AgentWorkKindV1,
    state: AgentWorkStateV1,
) -> Option<AgentWorkActionV1> {
    match state {
        AgentWorkStateV1::Satisfied | AgentWorkStateV1::Blocked => None,
        AgentWorkStateV1::Running => Some(AgentWorkActionV1::Wait),
        AgentWorkStateV1::NeedsReview => Some(AgentWorkActionV1::Review),
        AgentWorkStateV1::Ready => Some(match kind {
            AgentWorkKindV1::Content => AgentWorkActionV1::ProvideContent,
            AgentWorkKindV1::ScenePlan => AgentWorkActionV1::ProvideScenePlan,
            AgentWorkKindV1::Visual => AgentWorkActionV1::ProvideVisual,
            AgentWorkKindV1::Voice => AgentWorkActionV1::ProvideVoice,
            AgentWorkKindV1::ProductionPack => AgentWorkActionV1::AssembleProductionPack,
        }),
    }
}

fn message_v1(kind: AgentWorkKindV1, state: AgentWorkStateV1) -> String {
    let noun = match kind {
        AgentWorkKindV1::Content => "content",
        AgentWorkKindV1::ScenePlan => "scene plan",
        AgentWorkKindV1::Visual => "visual unit",
        AgentWorkKindV1::Voice => "voice unit",
        AgentWorkKindV1::ProductionPack => "production pack",
    };
    match state {
        AgentWorkStateV1::Blocked => format!("{noun} is blocked by canonical dependencies"),
        AgentWorkStateV1::Ready => format!("{noun} is ready for canonical or external execution"),
        AgentWorkStateV1::Running => format!("{noun} has canonical work in progress"),
        AgentWorkStateV1::NeedsReview => {
            format!("{noun} needs review or recovery before continuing")
        }
        AgentWorkStateV1::Satisfied => format!("{noun} is satisfied by verified canonical state"),
    }
}
