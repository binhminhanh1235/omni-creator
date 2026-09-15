use omnicreator_core::{
    ProductionRecoveryArtifactKindV1, ProductionRecoveryArtifactStateV1, ProductionRecoveryItemV1,
};
use serde::{Deserialize, Serialize};

use crate::{
    AgentWorkGraphV1, AgentWorkItemV1, AgentWorkKindV1, AgentWorkStateV1,
    ApplicationControlService, ControlResultV1, ProjectIdRequestV1,
};

pub const AGENT_FAN_IN_QA_SCHEMA_V1: &str = "omnicreator.agent-fan-in-qa";
pub const AGENT_FAN_IN_QA_VERSION_V1: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentFanInRecoveryActionV1 {
    Wait,
    DispatchExternal,
    Review,
    RepairVisual,
    RepairVoiceBundle,
    AssembleProductionPack,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentFanInUnitV1 {
    pub work_id: String,
    pub kind: AgentWorkKindV1,
    pub canonical_unit: String,
    pub state: AgentWorkStateV1,
    pub selected_artifact_ids: Vec<String>,
    pub recovery_items: Vec<ProductionRecoveryItemV1>,
    pub recovery_action: AgentFanInRecoveryActionV1,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentFanInQaV1 {
    pub schema: String,
    pub version: u32,
    pub project_id: String,
    pub read_only: bool,
    pub visual_units: Vec<AgentFanInUnitV1>,
    pub voice_units: Vec<AgentFanInUnitV1>,
    pub ready_work_ids: Vec<String>,
    pub needs_review_work_ids: Vec<String>,
    pub unhealthy_canonical_units: Vec<String>,
    pub recovery_ready_for_rebuild: bool,
    pub fan_in_verified: bool,
    pub production_pack_ready: bool,
    pub production_pack_satisfied: bool,
    pub suggested_action: AgentFanInRecoveryActionV1,
}

impl<'a> ApplicationControlService<'a> {
    /// Derives agent-facing fan-in QA from canonical work and recovery state.
    ///
    /// This projection is intentionally read-only. It does not create worker,
    /// claim, queue, recovery or scheduler state of its own.
    pub fn agent_fan_in_qa_v1(
        &self,
        request: &ProjectIdRequestV1,
    ) -> ControlResultV1<AgentFanInQaV1> {
        let graph = self.agent_work_graph_v1(request)?;
        derive_agent_fan_in_qa_v1(self, request, graph)
    }
}

fn derive_agent_fan_in_qa_v1(
    service: &ApplicationControlService<'_>,
    request: &ProjectIdRequestV1,
    graph: AgentWorkGraphV1,
) -> ControlResultV1<AgentFanInQaV1> {
    let scene_plan_satisfied = graph.items.iter().any(|item| {
        item.kind == AgentWorkKindV1::ScenePlan && item.state == AgentWorkStateV1::Satisfied
    });

    let (recovery_items, recovery_ready_for_rebuild) = if scene_plan_satisfied {
        let recovery = service.production_recovery_v1(request)?.data.recovery;
        (recovery.items, recovery.ready_for_rebuild)
    } else {
        (Vec::new(), false)
    };

    let visual_units = graph
        .items
        .iter()
        .filter(|item| item.kind == AgentWorkKindV1::Visual)
        .map(|item| fan_in_unit_v1(item, &recovery_items))
        .collect::<Vec<_>>();
    let voice_units = graph
        .items
        .iter()
        .filter(|item| item.kind == AgentWorkKindV1::Voice)
        .map(|item| fan_in_unit_v1(item, &recovery_items))
        .collect::<Vec<_>>();

    let mut unhealthy_canonical_units = visual_units
        .iter()
        .chain(&voice_units)
        .filter(|unit| {
            unit.state == AgentWorkStateV1::Satisfied
                && unit.recovery_items.iter().any(|item| {
                    item.state != ProductionRecoveryArtifactStateV1::Verified
                })
        })
        .map(|unit| unit.canonical_unit.clone())
        .collect::<Vec<_>>();
    unhealthy_canonical_units.sort();
    unhealthy_canonical_units.dedup();

    let ready_work_ids = visual_units
        .iter()
        .chain(&voice_units)
        .filter(|unit| unit.state == AgentWorkStateV1::Ready)
        .map(|unit| unit.work_id.clone())
        .collect::<Vec<_>>();
    let needs_review_work_ids = visual_units
        .iter()
        .chain(&voice_units)
        .filter(|unit| unit.state == AgentWorkStateV1::NeedsReview)
        .map(|unit| unit.work_id.clone())
        .collect::<Vec<_>>();

    let fan_in_verified = !visual_units.is_empty()
        && !voice_units.is_empty()
        && visual_units
            .iter()
            .chain(&voice_units)
            .all(|unit| unit.state == AgentWorkStateV1::Satisfied)
        && recovery_ready_for_rebuild;

    let production = graph
        .items
        .iter()
        .find(|item| item.kind == AgentWorkKindV1::ProductionPack)
        .expect("agent work graph always contains production-pack work");
    let production_pack_ready = production.state == AgentWorkStateV1::Ready;
    let production_pack_satisfied = production.state == AgentWorkStateV1::Satisfied;

    let suggested_action = if !needs_review_work_ids.is_empty()
        || !unhealthy_canonical_units.is_empty()
    {
        AgentFanInRecoveryActionV1::Review
    } else if production_pack_ready {
        AgentFanInRecoveryActionV1::AssembleProductionPack
    } else if !ready_work_ids.is_empty() {
        AgentFanInRecoveryActionV1::DispatchExternal
    } else {
        AgentFanInRecoveryActionV1::Wait
    };

    Ok(AgentFanInQaV1 {
        schema: AGENT_FAN_IN_QA_SCHEMA_V1.to_owned(),
        version: AGENT_FAN_IN_QA_VERSION_V1,
        project_id: graph.project_id,
        read_only: graph.read_only,
        visual_units,
        voice_units,
        ready_work_ids,
        needs_review_work_ids,
        unhealthy_canonical_units,
        recovery_ready_for_rebuild,
        fan_in_verified,
        production_pack_ready,
        production_pack_satisfied,
        suggested_action,
    })
}

fn fan_in_unit_v1(
    item: &AgentWorkItemV1,
    recovery_items: &[ProductionRecoveryItemV1],
) -> AgentFanInUnitV1 {
    let mut matching_recovery = recovery_items
        .iter()
        .filter(|recovery| recovery.canonical_id == item.canonical_unit)
        .filter(|recovery| match item.kind {
            AgentWorkKindV1::Visual => recovery.kind == ProductionRecoveryArtifactKindV1::Visual,
            AgentWorkKindV1::Voice => matches!(
                recovery.kind,
                ProductionRecoveryArtifactKindV1::Audio
                    | ProductionRecoveryArtifactKindV1::Timing
            ),
            _ => false,
        })
        .cloned()
        .collect::<Vec<_>>();
    matching_recovery.sort_by_key(|recovery| match recovery.kind {
        ProductionRecoveryArtifactKindV1::Visual => 0,
        ProductionRecoveryArtifactKindV1::Audio => 1,
        ProductionRecoveryArtifactKindV1::Timing => 2,
    });

    let artifacts_healthy = !matching_recovery.is_empty()
        && matching_recovery
            .iter()
            .all(|recovery| recovery.state == ProductionRecoveryArtifactStateV1::Verified);
    let recovery_action = match item.state {
        AgentWorkStateV1::Blocked | AgentWorkStateV1::Running => AgentFanInRecoveryActionV1::Wait,
        AgentWorkStateV1::Ready => AgentFanInRecoveryActionV1::DispatchExternal,
        AgentWorkStateV1::NeedsReview => AgentFanInRecoveryActionV1::Review,
        AgentWorkStateV1::Satisfied if artifacts_healthy => AgentFanInRecoveryActionV1::None,
        AgentWorkStateV1::Satisfied if item.kind == AgentWorkKindV1::Visual => {
            AgentFanInRecoveryActionV1::RepairVisual
        }
        AgentWorkStateV1::Satisfied if item.kind == AgentWorkKindV1::Voice => {
            AgentFanInRecoveryActionV1::RepairVoiceBundle
        }
        AgentWorkStateV1::Satisfied => AgentFanInRecoveryActionV1::Review,
    };

    let detail = if matching_recovery.is_empty() {
        item.message.clone()
    } else {
        let unhealthy = matching_recovery
            .iter()
            .filter(|recovery| recovery.state != ProductionRecoveryArtifactStateV1::Verified)
            .count();
        if unhealthy == 0 {
            format!("{} Canonical artifacts are verified.", item.message)
        } else if item.state == AgentWorkStateV1::Satisfied {
            format!(
                "{} {unhealthy} canonical recovery artifact(s) require repair.",
                item.message
            )
        } else {
            item.message.clone()
        }
    };

    AgentFanInUnitV1 {
        work_id: item.work_id.clone(),
        kind: item.kind,
        canonical_unit: item.canonical_unit.clone(),
        state: item.state,
        selected_artifact_ids: item.selected_artifact_ids.clone(),
        recovery_items: matching_recovery,
        recovery_action,
        detail,
    }
}
