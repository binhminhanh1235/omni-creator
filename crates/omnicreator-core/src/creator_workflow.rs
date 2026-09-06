use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    deterministic_input_hash, EffectiveStudioPackV1, Error, Project, Result, StateStore, StepStatus,
    WorkflowStep,
};

pub const CREATOR_WORKFLOW_PLAN_SCHEMA_V1: &str = "omnicreator.creator-workflow-plan";
pub const CREATOR_WORKFLOW_PLAN_VERSION_V1: u32 = 1;
pub const CREATOR_WORKFLOW_UNIT_PROJECT_V1: &str = "project";

pub const CREATOR_STEP_CONTENT_PREPARE_V1: &str = "content.prepare";
pub const CREATOR_STEP_SCENE_PLAN_V1: &str = "scene.plan";
pub const CREATOR_STEP_VISUAL_PREPARE_V1: &str = "visual.prepare";
pub const CREATOR_STEP_VOICE_PREPARE_V1: &str = "voice.prepare";
pub const CREATOR_STEP_PRODUCTION_PACK_V1: &str = "production.pack";

const CREATOR_WORKFLOW_STAGE_ORDER_V1: [&str; 5] = [
    CREATOR_STEP_CONTENT_PREPARE_V1,
    CREATOR_STEP_SCENE_PLAN_V1,
    CREATOR_STEP_VISUAL_PREPARE_V1,
    CREATOR_STEP_VOICE_PREPARE_V1,
    CREATOR_STEP_PRODUCTION_PACK_V1,
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreatorWorkflowStepPlanV1 {
    pub step: String,
    pub unit: String,
    pub input_hash: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreatorWorkflowPlanV1 {
    pub schema: String,
    pub schema_version: u32,
    pub project_id: String,
    pub studio_pack_id: String,
    pub script_version: i64,
    pub source_hash: String,
    pub steps: Vec<CreatorWorkflowStepPlanV1>,
    pub plan_hash: String,
}

impl CreatorWorkflowPlanV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != CREATOR_WORKFLOW_PLAN_SCHEMA_V1 {
            return Err(Error::InvalidContract(format!(
                "unsupported creator workflow plan schema {}",
                self.schema
            )));
        }
        if self.schema_version != CREATOR_WORKFLOW_PLAN_VERSION_V1 {
            return Err(Error::InvalidContract(format!(
                "unsupported creator workflow plan version {}",
                self.schema_version
            )));
        }
        if self.project_id.trim().is_empty() || self.studio_pack_id.trim().is_empty() {
            return Err(Error::InvalidContract(
                "creator workflow project_id and studio_pack_id must not be empty".to_owned(),
            ));
        }
        if self.script_version <= 0 {
            return Err(Error::InvalidContract(
                "creator workflow script_version must be positive".to_owned(),
            ));
        }
        if self.source_hash.len() != 64 || self.plan_hash.len() != 64 {
            return Err(Error::InvalidContract(
                "creator workflow hashes must be SHA-256 hex strings".to_owned(),
            ));
        }
        if self.steps.len() != CREATOR_WORKFLOW_STAGE_ORDER_V1.len() {
            return Err(Error::InvalidContract(format!(
                "creator workflow v1 must contain exactly {} semantic stages",
                CREATOR_WORKFLOW_STAGE_ORDER_V1.len()
            )));
        }

        let mut seen = BTreeSet::new();
        for (index, step) in self.steps.iter().enumerate() {
            let expected = CREATOR_WORKFLOW_STAGE_ORDER_V1[index];
            if step.step != expected {
                return Err(Error::InvalidContract(format!(
                    "creator workflow stage {index} must be {expected}, found {}",
                    step.step
                )));
            }
            if step.unit != CREATOR_WORKFLOW_UNIT_PROJECT_V1 {
                return Err(Error::InvalidContract(format!(
                    "creator workflow step {} must use project unit",
                    step.step
                )));
            }
            if step.input_hash.len() != 64 {
                return Err(Error::InvalidContract(format!(
                    "creator workflow step {} must have a SHA-256 input hash",
                    step.step
                )));
            }
            if !seen.insert(step.step.as_str()) {
                return Err(Error::InvalidContract(format!(
                    "duplicate creator workflow step {}",
                    step.step
                )));
            }

            let mut dependency_seen = BTreeSet::new();
            for dependency in &step.depends_on {
                if !dependency_seen.insert(dependency.as_str()) {
                    return Err(Error::InvalidContract(format!(
                        "creator workflow step {} contains duplicate dependency {}",
                        step.step, dependency
                    )));
                }
                if !seen.contains(dependency.as_str()) {
                    return Err(Error::InvalidContract(format!(
                        "creator workflow dependency {} for {} must refer to an earlier stage",
                        dependency, step.step
                    )));
                }
            }
        }

        let expected_hash = creator_workflow_plan_hash_v1(&self.source_hash, &self.steps);
        if self.plan_hash != expected_hash {
            return Err(Error::InvalidContract(
                "creator workflow plan hash does not match its semantic stages".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn canonical_json_v1(&self) -> Result<String> {
        self.validate_v1()?;
        Ok(serde_json::to_string(self)?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreatorWorkflowMaterializationV1 {
    pub plan_hash: String,
    pub steps: Vec<WorkflowStep>,
    pub created_step_ids: Vec<String>,
}

pub fn compile_creator_workflow_plan_v1(
    project: &Project,
    studio_pack: &EffectiveStudioPackV1,
) -> Result<CreatorWorkflowPlanV1> {
    studio_pack.validate_v1()?;
    if project.id.trim().is_empty() || project.title.trim().is_empty() {
        return Err(Error::InvalidContract(
            "creator workflow requires a persisted project with a non-empty title".to_owned(),
        ));
    }
    if project.script_version <= 0 {
        return Err(Error::InvalidContract(
            "creator workflow requires a positive project script_version".to_owned(),
        ));
    }
    if project.studio_pack.as_deref() != Some(studio_pack.id.as_str()) {
        return Err(Error::InvalidContract(format!(
            "project {} is bound to Studio Pack {:?}, not {}",
            project.id, project.studio_pack, studio_pack.id
        )));
    }

    let pack_json = studio_pack.canonical_json_v1()?;
    let script_version = project.script_version.to_string();
    let source_hash = deterministic_input_hash(&[
        b"creator-workflow-source-v1",
        project.id.as_bytes(),
        project.title.trim().as_bytes(),
        script_version.as_bytes(),
        pack_json.as_bytes(),
    ]);

    let stage_specs: [(&str, &[&str]); 5] = [
        (CREATOR_STEP_CONTENT_PREPARE_V1, &[]),
        (
            CREATOR_STEP_SCENE_PLAN_V1,
            &[CREATOR_STEP_CONTENT_PREPARE_V1],
        ),
        (
            CREATOR_STEP_VISUAL_PREPARE_V1,
            &[CREATOR_STEP_SCENE_PLAN_V1],
        ),
        (
            CREATOR_STEP_VOICE_PREPARE_V1,
            &[CREATOR_STEP_CONTENT_PREPARE_V1],
        ),
        (
            CREATOR_STEP_PRODUCTION_PACK_V1,
            &[
                CREATOR_STEP_VISUAL_PREPARE_V1,
                CREATOR_STEP_VOICE_PREPARE_V1,
            ],
        ),
    ];

    let steps = stage_specs
        .into_iter()
        .map(|(step, dependencies)| CreatorWorkflowStepPlanV1 {
            step: step.to_owned(),
            unit: CREATOR_WORKFLOW_UNIT_PROJECT_V1.to_owned(),
            input_hash: deterministic_input_hash(&[
                b"creator-workflow-step-v1",
                source_hash.as_bytes(),
                step.as_bytes(),
            ]),
            depends_on: dependencies
                .iter()
                .map(|dependency| (*dependency).to_owned())
                .collect(),
        })
        .collect::<Vec<_>>();

    let plan = CreatorWorkflowPlanV1 {
        schema: CREATOR_WORKFLOW_PLAN_SCHEMA_V1.to_owned(),
        schema_version: CREATOR_WORKFLOW_PLAN_VERSION_V1,
        project_id: project.id.clone(),
        studio_pack_id: studio_pack.id.clone(),
        script_version: project.script_version,
        source_hash: source_hash.clone(),
        plan_hash: creator_workflow_plan_hash_v1(&source_hash, &steps),
        steps,
    };
    plan.validate_v1()?;
    Ok(plan)
}

pub fn materialize_creator_workflow_plan_v1(
    store: &StateStore,
    plan: &CreatorWorkflowPlanV1,
) -> Result<CreatorWorkflowMaterializationV1> {
    plan.validate_v1()?;
    let project = store.get_project(&plan.project_id)?;
    if project.studio_pack.as_deref() != Some(plan.studio_pack_id.as_str()) {
        return Err(Error::InvalidContract(format!(
            "project {} Studio Pack binding changed after creator workflow compilation",
            plan.project_id
        )));
    }
    if project.script_version != plan.script_version {
        return Err(Error::InvalidContract(format!(
            "project {} script_version changed after creator workflow compilation",
            plan.project_id
        )));
    }

    let mut existing = store
        .list_project_steps(&plan.project_id)?
        .into_iter()
        .map(|step| ((step.step.clone(), step.unit.clone()), step))
        .collect::<BTreeMap<_, _>>();

    let mut ordered_steps = Vec::with_capacity(plan.steps.len());
    let mut created_step_ids = Vec::new();

    for planned in &plan.steps {
        let identity = (planned.step.clone(), planned.unit.clone());
        let materialized = if let Some(step) = existing.get(&identity) {
            if step.input_hash.as_deref() != Some(planned.input_hash.as_str()) {
                return Err(Error::InvalidContract(format!(
                    "existing workflow step {}/{} has a conflicting input hash; invalidate or replan before materializing",
                    planned.step, planned.unit
                )));
            }
            step.clone()
        } else {
            let status = if planned.depends_on.is_empty() {
                StepStatus::Ready
            } else {
                StepStatus::NotReady
            };
            let step = store.create_step(
                &plan.project_id,
                &planned.step,
                &planned.unit,
                status,
                Some(&planned.input_hash),
            )?;
            created_step_ids.push(step.step_id.clone());
            existing.insert(identity, step.clone());
            step
        };
        ordered_steps.push(materialized);
    }

    for planned in &plan.steps {
        let downstream = existing
            .get(&(planned.step.clone(), planned.unit.clone()))
            .ok_or_else(|| {
                Error::InvalidContract(format!(
                    "materialized creator workflow is missing step {}",
                    planned.step
                ))
            })?;
        for dependency in &planned.depends_on {
            let upstream = existing
                .get(&(dependency.clone(), CREATOR_WORKFLOW_UNIT_PROJECT_V1.to_owned()))
                .ok_or_else(|| {
                    Error::InvalidContract(format!(
                        "materialized creator workflow is missing dependency {}",
                        dependency
                    ))
                })?;
            store.add_dependency(&upstream.step_id, &downstream.step_id)?;
        }
    }

    Ok(CreatorWorkflowMaterializationV1 {
        plan_hash: plan.plan_hash.clone(),
        steps: ordered_steps,
        created_step_ids,
    })
}

fn creator_workflow_plan_hash_v1(
    source_hash: &str,
    steps: &[CreatorWorkflowStepPlanV1],
) -> String {
    let step_fingerprint = steps
        .iter()
        .map(|step| {
            format!(
                "{}\0{}\0{}\0{}",
                step.step,
                step.unit,
                step.input_hash,
                step.depends_on.join(",")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    deterministic_input_hash(&[
        b"creator-workflow-plan-v1",
        source_hash.as_bytes(),
        step_fingerprint.as_bytes(),
    ])
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::initial_studio_pack_catalog_v1;

    fn fixture() -> (tempfile::TempDir, StateStore, Project, EffectiveStudioPackV1) {
        let temp = tempdir().unwrap();
        let store = StateStore::open(temp.path().join("state.db")).unwrap();
        let pack = initial_studio_pack_catalog_v1()
            .unwrap()
            .resolve_v1("christian-cinematic")
            .unwrap();
        let project = store
            .create_project_with_studio_pack("When God Seems Silent", Some(&pack.id))
            .unwrap();
        (temp, store, project, pack)
    }

    #[test]
    fn creator_workflow_plan_is_deterministic_and_provider_neutral() {
        let (_temp, _store, project, pack) = fixture();
        let first = compile_creator_workflow_plan_v1(&project, &pack).unwrap();
        let second = compile_creator_workflow_plan_v1(&project, &pack).unwrap();

        assert_eq!(first, second);
        assert_eq!(
            first
                .steps
                .iter()
                .map(|step| step.step.as_str())
                .collect::<Vec<_>>(),
            CREATOR_WORKFLOW_STAGE_ORDER_V1
        );
        assert_eq!(
            first.steps[4].depends_on,
            vec![
                CREATOR_STEP_VISUAL_PREPARE_V1.to_owned(),
                CREATOR_STEP_VOICE_PREPARE_V1.to_owned()
            ]
        );

        let json = first.canonical_json_v1().unwrap();
        for forbidden in [
            "/Users/",
            "/home/",
            "api_key",
            "secret",
            "endpoint",
            "provider_id",
            "model_id",
        ] {
            assert!(!json.contains(forbidden), "found forbidden token {forbidden}");
        }
    }

    #[test]
    fn creator_workflow_plan_rejects_studio_pack_binding_mismatch() {
        let (_temp, _store, mut project, pack) = fixture();
        project.studio_pack = Some("other-pack".to_owned());

        assert!(matches!(
            compile_creator_workflow_plan_v1(&project, &pack),
            Err(Error::InvalidContract(message)) if message.contains("not")
        ));
    }

    #[test]
    fn materialization_uses_existing_canonical_steps_and_is_idempotent() {
        let (_temp, store, project, pack) = fixture();
        let plan = compile_creator_workflow_plan_v1(&project, &pack).unwrap();

        let first = materialize_creator_workflow_plan_v1(&store, &plan).unwrap();
        assert_eq!(first.created_step_ids.len(), 5);
        assert_eq!(first.steps[0].status, StepStatus::Ready);
        assert!(first.steps[1..]
            .iter()
            .all(|step| step.status == StepStatus::NotReady));

        let downstream = store.downstream_steps(&first.steps[0].step_id).unwrap();
        assert_eq!(downstream.len(), 5);

        let second = materialize_creator_workflow_plan_v1(&store, &plan).unwrap();
        assert!(second.created_step_ids.is_empty());
        assert_eq!(
            first
                .steps
                .iter()
                .map(|step| step.step_id.as_str())
                .collect::<Vec<_>>(),
            second
                .steps
                .iter()
                .map(|step| step.step_id.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn materialization_rejects_conflicting_existing_step_hash() {
        let (_temp, store, project, pack) = fixture();
        let plan = compile_creator_workflow_plan_v1(&project, &pack).unwrap();
        store
            .create_step(
                &project.id,
                CREATOR_STEP_CONTENT_PREPARE_V1,
                CREATOR_WORKFLOW_UNIT_PROJECT_V1,
                StepStatus::Ready,
                Some("different"),
            )
            .unwrap();

        assert!(matches!(
            materialize_creator_workflow_plan_v1(&store, &plan),
            Err(Error::InvalidContract(message)) if message.contains("conflicting input hash")
        ));
    }

    #[test]
    fn plan_hash_detects_semantic_tampering() {
        let (_temp, _store, project, pack) = fixture();
        let mut plan = compile_creator_workflow_plan_v1(&project, &pack).unwrap();
        plan.steps[4].depends_on.pop();

        assert!(matches!(
            plan.validate_v1(),
            Err(Error::InvalidContract(message)) if message.contains("plan hash")
        ));
    }
}
