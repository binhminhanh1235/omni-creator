use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::{
    Attempt, EffectiveStudioPackV1, Error, Job, Project, Result, StepStatus,
    StudioAutomationLevelV1, StudioPackAvailabilityReasonCodeV1,
    StudioPackAvailabilityReasonV1, StudioPackAvailabilityStatusV1, StudioPackAvailabilityV1,
    StudioPackRouteTargetV1, StudioPackV1, WorkflowStep,
};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StudioPackValueSourceV1 {
    Default,
    Inherited,
    ExplicitOverride,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StudioAutomationPolicyV1 {
    pub level: StudioAutomationLevelV1,
    pub review_major_stages: bool,
    pub auto_advance_low_risk: bool,
    pub route_ambiguous_to_review: bool,
    pub stop_on_blocking_exception: bool,
    pub stop_on_high_impact_exception: bool,
}

pub fn studio_automation_policy_v1(level: StudioAutomationLevelV1) -> StudioAutomationPolicyV1 {
    match level {
        StudioAutomationLevelV1::Assisted => StudioAutomationPolicyV1 {
            level,
            review_major_stages: true,
            auto_advance_low_risk: false,
            route_ambiguous_to_review: true,
            stop_on_blocking_exception: true,
            stop_on_high_impact_exception: true,
        },
        StudioAutomationLevelV1::Balanced => StudioAutomationPolicyV1 {
            level,
            review_major_stages: false,
            auto_advance_low_risk: true,
            route_ambiguous_to_review: true,
            stop_on_blocking_exception: true,
            stop_on_high_impact_exception: true,
        },
        StudioAutomationLevelV1::Autopilot => StudioAutomationPolicyV1 {
            level,
            review_major_stages: false,
            auto_advance_low_risk: true,
            route_ambiguous_to_review: false,
            stop_on_blocking_exception: true,
            stop_on_high_impact_exception: true,
        },
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StudioPackAutomationViewV1 {
    pub value: StudioAutomationLevelV1,
    pub source: StudioPackValueSourceV1,
    pub policy: StudioAutomationPolicyV1,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StudioPackPresetViewV1 {
    pub key: String,
    pub value: String,
    pub source: StudioPackValueSourceV1,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StudioPackQualityViewV1 {
    pub key: String,
    pub value: u8,
    pub source: StudioPackValueSourceV1,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StudioPackRouteViewV1 {
    pub key: String,
    pub targets: Vec<StudioPackRouteTargetV1>,
    pub source: StudioPackValueSourceV1,
    pub availability_reasons: Vec<StudioPackAvailabilityReasonV1>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StudioPackUxViewV1 {
    pub id: String,
    pub name: String,
    pub lineage: Vec<String>,
    pub availability: StudioPackAvailabilityV1,
    pub automation: StudioPackAutomationViewV1,
    pub presets: Vec<StudioPackPresetViewV1>,
    pub quality_thresholds: Vec<StudioPackQualityViewV1>,
    pub routes: Vec<StudioPackRouteViewV1>,
}

pub fn build_studio_pack_ux_view_v1(
    definition: &StudioPackV1,
    effective: &EffectiveStudioPackV1,
    availability: &StudioPackAvailabilityV1,
) -> Result<StudioPackUxViewV1> {
    definition.validate_v1()?;
    effective.validate_v1()?;
    if definition.id != effective.id || availability.pack_id != effective.id {
        return Err(Error::InvalidContract(
            "Studio Pack UX projection requires matching definition/effective/availability ids"
                .to_owned(),
        ));
    }

    let automation_source = if definition.overrides.automation_level.is_some() {
        StudioPackValueSourceV1::ExplicitOverride
    } else if definition.extends.is_some() {
        StudioPackValueSourceV1::Inherited
    } else {
        StudioPackValueSourceV1::Default
    };
    let automation = StudioPackAutomationViewV1 {
        value: effective.config.automation_level,
        source: automation_source,
        policy: studio_automation_policy_v1(effective.config.automation_level),
    };

    let presets = effective
        .config
        .presets
        .iter()
        .map(|(key, value)| StudioPackPresetViewV1 {
            key: key.clone(),
            value: value.clone(),
            source: value_source_v1(
                definition.extends.is_some(),
                definition.overrides.presets.contains_key(key),
            ),
        })
        .collect();

    let quality_thresholds = effective
        .config
        .quality_thresholds
        .iter()
        .map(|(key, value)| StudioPackQualityViewV1 {
            key: key.clone(),
            value: *value,
            source: value_source_v1(
                definition.extends.is_some(),
                definition.overrides.quality_thresholds.contains_key(key),
            ),
        })
        .collect();

    let reasons_by_route = availability
        .reasons
        .iter()
        .fold(BTreeMap::<String, Vec<StudioPackAvailabilityReasonV1>>::new(), |mut map, reason| {
            map.entry(reason.route.clone()).or_default().push(reason.clone());
            map
        });

    let routes = effective
        .config
        .routes
        .iter()
        .map(|(key, route)| StudioPackRouteViewV1 {
            key: key.clone(),
            targets: route.targets.clone(),
            source: value_source_v1(
                definition.extends.is_some(),
                definition.overrides.routes.contains_key(key),
            ),
            availability_reasons: reasons_by_route.get(key).cloned().unwrap_or_default(),
        })
        .collect();

    Ok(StudioPackUxViewV1 {
        id: effective.id.clone(),
        name: effective.name.clone(),
        lineage: effective.lineage.clone(),
        availability: availability.clone(),
        automation,
        presets,
        quality_thresholds,
        routes,
    })
}

fn value_source_v1(has_parent: bool, explicit: bool) -> StudioPackValueSourceV1 {
    if explicit {
        StudioPackValueSourceV1::ExplicitOverride
    } else if has_parent {
        StudioPackValueSourceV1::Inherited
    } else {
        StudioPackValueSourceV1::Default
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StudioReviewSeverityV1 {
    Info,
    ActionRequired,
    Blocking,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StudioReviewKindV1 {
    FailedOrRetryableJob,
    MissingCapability,
    SetupRequirement,
    BlockedWorkflow,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StudioReviewActionV1 {
    RetryJob { job_id: String },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StudioReviewItemV1 {
    pub id: String,
    pub project_id: String,
    pub project_title: String,
    pub kind: StudioReviewKindV1,
    pub severity: StudioReviewSeverityV1,
    pub reason: String,
    pub canonical_source: String,
    pub source_id: String,
    pub action: Option<StudioReviewActionV1>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StudioReviewCenterV1 {
    pub items: Vec<StudioReviewItemV1>,
    pub blocking_count: usize,
    pub actionable_count: usize,
}

#[derive(Debug, Clone)]
pub struct StudioJobReviewSnapshotV1 {
    pub job: Job,
    pub attempts: Vec<Attempt>,
}

pub fn build_studio_review_center_v1(
    projects: &[(Project, Vec<StudioJobReviewSnapshotV1>, Vec<WorkflowStep>, Option<StudioPackAvailabilityV1>)],
) -> StudioReviewCenterV1 {
    let mut items = Vec::new();
    for (project, jobs, steps, availability) in projects {
        append_pack_review_items_v1(&mut items, project, availability.as_ref());
        append_job_review_items_v1(&mut items, project, jobs);
        append_step_review_items_v1(&mut items, project, steps);
    }
    items.sort_by(|left, right| {
        right
            .severity
            .cmp(&left.severity)
            .then_with(|| left.project_title.cmp(&right.project_title))
            .then_with(|| left.id.cmp(&right.id))
    });
    let blocking_count = items
        .iter()
        .filter(|item| item.severity == StudioReviewSeverityV1::Blocking)
        .count();
    let actionable_count = items.iter().filter(|item| item.action.is_some()).count();
    StudioReviewCenterV1 {
        items,
        blocking_count,
        actionable_count,
    }
}

fn append_pack_review_items_v1(
    items: &mut Vec<StudioReviewItemV1>,
    project: &Project,
    availability: Option<&StudioPackAvailabilityV1>,
) {
    let Some(availability) = availability else {
        return;
    };
    if availability.status == StudioPackAvailabilityStatusV1::Available {
        return;
    }

    let mut seen = BTreeSet::new();
    for reason in &availability.reasons {
        if !reason.blocking && reason.code != StudioPackAvailabilityReasonCodeV1::SetupRequired {
            continue;
        }
        let (kind, severity) = match reason.code {
            StudioPackAvailabilityReasonCodeV1::SetupRequired => (
                StudioReviewKindV1::SetupRequirement,
                StudioReviewSeverityV1::Blocking,
            ),
            StudioPackAvailabilityReasonCodeV1::RequiredCapabilityMissing
            | StudioPackAvailabilityReasonCodeV1::PluginUnavailable => (
                StudioReviewKindV1::MissingCapability,
                StudioReviewSeverityV1::Blocking,
            ),
            StudioPackAvailabilityReasonCodeV1::PreferredPluginMissing
            | StudioPackAvailabilityReasonCodeV1::OptionalFallbackUnavailable => continue,
        };
        let source_id = format!("{}:{}", availability.pack_id, reason.route);
        if !seen.insert((kind as u8, source_id.clone())) {
            continue;
        }
        let plugin = reason
            .plugin_id
            .as_deref()
            .map(|id| format!(" via {id}"))
            .unwrap_or_default();
        let runtime = reason
            .runtime_reason
            .as_deref()
            .map(|code| format!(" ({code})"))
            .unwrap_or_default();
        items.push(StudioReviewItemV1 {
            id: format!("pack:{}:{}", project.id, source_id),
            project_id: project.id.clone(),
            project_title: project.title.clone(),
            kind,
            severity,
            reason: format!(
                "{} requires capability '{}'{}{}.",
                reason.route, reason.capability, plugin, runtime
            ),
            canonical_source: "PluginRegistry + StudioPack availability".to_owned(),
            source_id,
            action: None,
        });
    }
}

fn append_job_review_items_v1(
    items: &mut Vec<StudioReviewItemV1>,
    project: &Project,
    jobs: &[StudioJobReviewSnapshotV1],
) {
    for snapshot in jobs {
        let job = &snapshot.job;
        let (severity, action) = match job.status {
            StepStatus::Retryable | StepStatus::Failed => (
                StudioReviewSeverityV1::ActionRequired,
                Some(StudioReviewActionV1::RetryJob {
                    job_id: job.job_id.clone(),
                }),
            ),
            StepStatus::Fatal => (StudioReviewSeverityV1::Blocking, None),
            _ => continue,
        };
        let last_error = snapshot
            .attempts
            .iter()
            .rev()
            .find_map(|attempt| attempt.error_code.as_deref())
            .map(|error| format!(" Last error: {error}."))
            .unwrap_or_default();
        items.push(StudioReviewItemV1 {
            id: format!("job:{}:{}", project.id, job.job_id),
            project_id: project.id.clone(),
            project_title: project.title.clone(),
            kind: StudioReviewKindV1::FailedOrRetryableJob,
            severity,
            reason: format!(
                "{} / {} is {}.{}",
                job.step,
                job.unit,
                job.status.as_str(),
                last_error
            ),
            canonical_source: "Job / Attempt".to_owned(),
            source_id: job.job_id.clone(),
            action,
        });
    }
}

fn append_step_review_items_v1(
    items: &mut Vec<StudioReviewItemV1>,
    project: &Project,
    steps: &[WorkflowStep],
) {
    for step in steps {
        if step.status != StepStatus::NotReady {
            continue;
        }
        items.push(StudioReviewItemV1 {
            id: format!("step:{}:{}", project.id, step.step_id),
            project_id: project.id.clone(),
            project_title: project.title.clone(),
            kind: StudioReviewKindV1::BlockedWorkflow,
            severity: StudioReviewSeverityV1::Info,
            reason: format!("{} / {} is waiting on canonical dependencies.", step.step, step.unit),
            canonical_source: "WorkflowStep".to_owned(),
            source_id: step.step_id.clone(),
            action: None,
        });
    }
}

pub fn studio_automation_allows_progress_v1(
    level: StudioAutomationLevelV1,
    has_ambiguous_decision: bool,
    has_high_impact_exception: bool,
    review: &StudioReviewCenterV1,
) -> bool {
    let policy = studio_automation_policy_v1(level);
    if policy.stop_on_blocking_exception && review.blocking_count > 0 {
        return false;
    }
    if policy.stop_on_high_impact_exception && has_high_impact_exception {
        return false;
    }
    if level == StudioAutomationLevelV1::Assisted {
        return false;
    }
    if policy.route_ambiguous_to_review && has_ambiguous_decision {
        return false;
    }
    policy.auto_advance_low_risk
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::{
        StudioPackAvailabilityReasonV1, StudioPackAvailabilityStatusV1, StudioPackCatalogV1,
        StudioPackOverridesV1, STUDIO_PACK_SCHEMA_V1, STUDIO_PACK_VERSION_V1,
    };

    fn project() -> Project {
        Project {
            id: "prj-1".to_owned(),
            title: "Test project".to_owned(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            studio_pack: Some("child".to_owned()),
            channel_profile: None,
            script_version: 1,
            production_lock: false,
        }
    }

    #[test]
    fn automation_levels_map_deterministically() {
        let assisted = studio_automation_policy_v1(StudioAutomationLevelV1::Assisted);
        let balanced = studio_automation_policy_v1(StudioAutomationLevelV1::Balanced);
        let autopilot = studio_automation_policy_v1(StudioAutomationLevelV1::Autopilot);

        assert!(assisted.review_major_stages);
        assert!(!assisted.auto_advance_low_risk);
        assert!(balanced.auto_advance_low_risk);
        assert!(balanced.route_ambiguous_to_review);
        assert!(autopilot.auto_advance_low_risk);
        assert!(!autopilot.route_ambiguous_to_review);
        assert!(autopilot.stop_on_blocking_exception);
    }

    #[test]
    fn projection_distinguishes_inherited_and_explicit_values() {
        let base = StudioPackV1 {
            schema: STUDIO_PACK_SCHEMA_V1.to_owned(),
            schema_version: STUDIO_PACK_VERSION_V1,
            id: "base".to_owned(),
            name: "Base".to_owned(),
            extends: None,
            overrides: StudioPackOverridesV1 {
                automation_level: Some(StudioAutomationLevelV1::Balanced),
                presets: BTreeMap::from([("visual_style".to_owned(), "cinematic".to_owned())]),
                ..Default::default()
            },
        };
        let child = StudioPackV1 {
            schema: STUDIO_PACK_SCHEMA_V1.to_owned(),
            schema_version: STUDIO_PACK_VERSION_V1,
            id: "child".to_owned(),
            name: "Child".to_owned(),
            extends: Some("base".to_owned()),
            overrides: StudioPackOverridesV1 {
                presets: BTreeMap::from([("pacing".to_owned(), "slow".to_owned())]),
                ..Default::default()
            },
        };
        let catalog = StudioPackCatalogV1::from_packs_v1(vec![base, child.clone()]).unwrap();
        let effective = catalog.resolve_v1("child").unwrap();
        let availability = StudioPackAvailabilityV1 {
            pack_id: "child".to_owned(),
            status: StudioPackAvailabilityStatusV1::Available,
            reasons: Vec::new(),
        };

        let view = build_studio_pack_ux_view_v1(&child, &effective, &availability).unwrap();

        assert_eq!(view.automation.source, StudioPackValueSourceV1::Inherited);
        assert_eq!(
            view.presets
                .iter()
                .find(|preset| preset.key == "visual_style")
                .unwrap()
                .source,
            StudioPackValueSourceV1::Inherited
        );
        assert_eq!(
            view.presets
                .iter()
                .find(|preset| preset.key == "pacing")
                .unwrap()
                .source,
            StudioPackValueSourceV1::ExplicitOverride
        );
    }

    #[test]
    fn missing_capability_is_blocking_and_stops_autopilot() {
        let availability = StudioPackAvailabilityV1 {
            pack_id: "child".to_owned(),
            status: StudioPackAvailabilityStatusV1::Unavailable,
            reasons: vec![StudioPackAvailabilityReasonV1 {
                code: StudioPackAvailabilityReasonCodeV1::RequiredCapabilityMissing,
                route: "visual.conceptual".to_owned(),
                plugin_type: "visual".to_owned(),
                capability: "stick_figure_visual".to_owned(),
                plugin_id: None,
                runtime_reason: None,
                blocking: true,
            }],
        };
        let review = build_studio_review_center_v1(&[(
            project(),
            Vec::new(),
            Vec::new(),
            Some(availability),
        )]);

        assert_eq!(review.blocking_count, 1);
        assert!(!studio_automation_allows_progress_v1(
            StudioAutomationLevelV1::Autopilot,
            false,
            false,
            &review,
        ));
    }

    #[test]
    fn resolved_job_disappears_without_review_state_storage() {
        let job = Job {
            job_id: "job-1".to_owned(),
            project_id: "prj-1".to_owned(),
            step: "voice".to_owned(),
            unit: "S01".to_owned(),
            status: StepStatus::Succeeded,
            input_hash: "hash".to_owned(),
            selected_attempt: None,
            selected_artifact: None,
        };
        let review = build_studio_review_center_v1(&[(
            project(),
            vec![StudioJobReviewSnapshotV1 {
                job,
                attempts: Vec::new(),
            }],
            Vec::new(),
            None,
        )]);

        assert!(review.items.is_empty());
        assert_eq!(review.blocking_count, 0);
    }
}
