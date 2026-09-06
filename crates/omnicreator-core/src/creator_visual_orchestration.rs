use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    build_visual_review, rank_visual_candidates, route_scene_generated_preference_v1,
    route_scene_visual_v1, CreatorContentV1, CreatorScenePlanV1, EffectiveStudioPackV1, Error,
    Project, RankedVisualCandidate, Result, SceneIntentV1, StockDiscoveryStatusV1,
    StudioAutomationLevelV1, StudioPackRouteTargetV1, VisualCandidate, VisualCandidateRankingInput,
    VisualMediaType, VisualRankingPolicy, VisualReviewOptions, VisualReviewSet,
    VisualRouteV1, VisualRoutingDecisionV1, VisualRoutingPolicyV1,
    STICK_FIGURE_VISUAL_CAPABILITY_V1,
};

pub const CREATOR_VISUAL_PLAN_SCHEMA_V1: &str = "omnicreator.creator-visual-plan";
pub const CREATOR_VISUAL_PLAN_VERSION_V1: u32 = 1;
pub const STOCK_IMAGE_CAPABILITY_V1: &str = "stock_image";
pub const STOCK_VIDEO_CAPABILITY_V1: &str = "stock_video";
pub const GENERATED_STILL_CAPABILITY_ROUTE_V1: &str = "generated_still";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CreatorStockDiscoveryV1 {
    pub status: StockDiscoveryStatusV1,
    #[serde(default)]
    pub ranking_inputs: Vec<VisualCandidateRankingInput>,
}

impl CreatorStockDiscoveryV1 {
    pub fn validate_v1(&self, scene: &SceneIntentV1) -> Result<()> {
        scene.validate_v1()?;
        if self.status == StockDiscoveryStatusV1::Unavailable && !self.ranking_inputs.is_empty() {
            return Err(Error::InvalidContract(
                "unavailable stock discovery must not contain candidates".to_owned(),
            ));
        }
        for input in &self.ranking_inputs {
            input.candidate.validate()?;
            input.signals.validate()?;
            if input.candidate.scene_id != scene.id {
                return Err(Error::InvalidContract(format!(
                    "stock discovery candidate {} belongs to scene {}, expected {}",
                    input.candidate.candidate_id, input.candidate.scene_id, scene.id
                )));
            }
        }
        Ok(())
    }
}

pub trait CreatorVisualDiscoveryExecutorV1 {
    fn discover_stock_v1(
        &self,
        scene: &SceneIntentV1,
        ordered_targets: &[StudioPackRouteTargetV1],
    ) -> Result<CreatorStockDiscoveryV1>;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CreatorVisualActionV1 {
    AwaitingStockSelection,
    FetchSelectedStock,
    AwaitingGenerationApproval,
    Generate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CreatorVisualScenePlanV1 {
    pub scene_id: String,
    pub route_key: String,
    pub route_targets: Vec<StudioPackRouteTargetV1>,
    pub routing: VisualRoutingDecisionV1,
    #[serde(default)]
    pub ranked_stock: Vec<RankedVisualCandidate>,
    pub review: Option<VisualReviewSet>,
    pub action: CreatorVisualActionV1,
    pub selected_candidate_id: Option<String>,
    pub execution_target: Option<StudioPackRouteTargetV1>,
}

impl CreatorVisualScenePlanV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.scene_id.trim().is_empty() || self.route_key.trim().is_empty() {
            return Err(Error::InvalidContract(
                "creator visual scene_id and route_key must not be empty".to_owned(),
            ));
        }
        if self.route_targets.is_empty() {
            return Err(Error::InvalidContract(
                "creator visual scene plan requires at least one route target".to_owned(),
            ));
        }
        for target in &self.route_targets {
            target.validate_v1()?;
            if target.plugin_type != "visual" {
                return Err(Error::InvalidContract(format!(
                    "creator visual route target must use visual plugin type, found {}",
                    target.plugin_type
                )));
            }
        }
        self.routing.validate_v1()?;
        if self.routing.scene_id != self.scene_id {
            return Err(Error::InvalidContract(
                "creator visual routing scene_id mismatch".to_owned(),
            ));
        }
        for ranked in &self.ranked_stock {
            ranked.candidate.validate()?;
            if ranked.candidate.scene_id != self.scene_id {
                return Err(Error::InvalidContract(
                    "creator visual ranked candidate scene_id mismatch".to_owned(),
                ));
            }
        }

        match self.action {
            CreatorVisualActionV1::AwaitingStockSelection => {
                if self.routing.route != VisualRouteV1::StockReview
                    || self.review.is_none()
                    || self.selected_candidate_id.is_some()
                    || self.execution_target.is_some()
                {
                    return Err(Error::InvalidContract(
                        "awaiting stock selection requires a stock review without a resolved selection"
                            .to_owned(),
                    ));
                }
            }
            CreatorVisualActionV1::FetchSelectedStock => {
                let selected = self.selected_candidate_id.as_deref().ok_or_else(|| {
                    Error::InvalidContract(
                        "selected stock execution requires selected_candidate_id".to_owned(),
                    )
                })?;
                if self.routing.route != VisualRouteV1::StockReview
                    || self.review.is_none()
                    || !self
                        .ranked_stock
                        .iter()
                        .any(|candidate| candidate.candidate.candidate_id == selected)
                    || self.execution_target.as_ref().is_none_or(|target| !is_stock_target_v1(target))
                {
                    return Err(Error::InvalidContract(
                        "selected stock execution is inconsistent with the ranked review set"
                            .to_owned(),
                    ));
                }
            }
            CreatorVisualActionV1::AwaitingGenerationApproval
            | CreatorVisualActionV1::Generate => {
                if self.routing.route != VisualRouteV1::GeneratedStill
                    || self.review.is_some()
                    || self.selected_candidate_id.is_some()
                    || self
                        .execution_target
                        .as_ref()
                        .is_none_or(|target| !is_generation_target_v1(target))
                {
                    return Err(Error::InvalidContract(
                        "generated execution requires a generated/stick route target and no stock selection"
                            .to_owned(),
                    ));
                }
            }
        }

        Ok(())
    }

    pub fn selected_candidate_v1(&self) -> Option<&VisualCandidate> {
        let selected = self.selected_candidate_id.as_deref()?;
        self.ranked_stock
            .iter()
            .find(|candidate| candidate.candidate.candidate_id == selected)
            .map(|candidate| &candidate.candidate)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CreatorVisualPlanV1 {
    pub schema: String,
    pub schema_version: u32,
    pub project_id: String,
    pub studio_pack_id: String,
    pub scene_plan_sha256: String,
    pub automation_level: StudioAutomationLevelV1,
    pub scenes: Vec<CreatorVisualScenePlanV1>,
}

impl CreatorVisualPlanV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != CREATOR_VISUAL_PLAN_SCHEMA_V1
            || self.schema_version != CREATOR_VISUAL_PLAN_VERSION_V1
        {
            return Err(Error::InvalidContract(
                "unsupported creator visual plan schema/version".to_owned(),
            ));
        }
        if self.project_id.trim().is_empty() || self.studio_pack_id.trim().is_empty() {
            return Err(Error::InvalidContract(
                "creator visual project_id and studio_pack_id must not be empty".to_owned(),
            ));
        }
        if !is_sha256_hex_v1(&self.scene_plan_sha256) {
            return Err(Error::InvalidContract(
                "creator visual scene_plan_sha256 must be lowercase SHA-256 hex".to_owned(),
            ));
        }
        if self.scenes.is_empty() {
            return Err(Error::InvalidContract(
                "creator visual plan must contain at least one scene".to_owned(),
            ));
        }
        let mut seen = BTreeSet::new();
        for scene in &self.scenes {
            scene.validate_v1()?;
            if !seen.insert(scene.scene_id.as_str()) {
                return Err(Error::InvalidContract(format!(
                    "duplicate creator visual scene {}",
                    scene.scene_id
                )));
            }
        }
        Ok(())
    }
}

pub fn plan_creator_visuals_v1(
    project: &Project,
    studio_pack: &EffectiveStudioPackV1,
    content: &CreatorContentV1,
    scene_plan: &CreatorScenePlanV1,
    scene_plan_sha256: &str,
    discovery: &impl CreatorVisualDiscoveryExecutorV1,
    ranking_policy: &VisualRankingPolicy,
    review_options: VisualReviewOptions,
) -> Result<CreatorVisualPlanV1> {
    studio_pack.validate_v1()?;
    content.validate_v1()?;
    scene_plan.validate_v1(content)?;
    ranking_policy.validate()?;
    review_options.validate()?;
    if project.id != content.project_id || project.id != scene_plan.project_id {
        return Err(Error::InvalidContract(
            "creator visual Project/content/scene-plan ids must match".to_owned(),
        ));
    }
    if project.studio_pack.as_deref() != Some(studio_pack.id.as_str()) {
        return Err(Error::InvalidContract(
            "creator visual Project Studio Pack binding does not match resolved pack".to_owned(),
        ));
    }
    if !is_sha256_hex_v1(scene_plan_sha256) {
        return Err(Error::InvalidContract(
            "creator visual scene_plan_sha256 must be lowercase SHA-256 hex".to_owned(),
        ));
    }

    let routing_policy = routing_policy_from_studio_pack_v1(studio_pack)?;
    let mut scenes = Vec::with_capacity(scene_plan.scenes.len());

    for scene in &scene_plan.scenes {
        let route_key = visual_route_key_v1(scene)?;
        let route = studio_pack.config.routes.get(&route_key).ok_or_else(|| {
            Error::InvalidContract(format!(
                "Studio Pack {} has no route for {route_key}",
                studio_pack.id
            ))
        })?;
        route.validate_v1()?;

        let route_targets = route.targets.clone();
        let generation_index = route_targets
            .iter()
            .position(is_generation_target_v1);
        let stock_prefix_end = generation_index.unwrap_or(route_targets.len());
        let stock_targets = route_targets[..stock_prefix_end]
            .iter()
            .filter(|target| is_stock_target_v1(target))
            .cloned()
            .collect::<Vec<_>>();

        let (routing, ranked_stock, review, selected_candidate_id, execution_target, action) =
            if generation_index == Some(0) {
                let routing = route_scene_generated_preference_v1(scene)?;
                let target = route_targets[0].clone();
                let action = match studio_pack.config.automation_level {
                    StudioAutomationLevelV1::Assisted => {
                        CreatorVisualActionV1::AwaitingGenerationApproval
                    }
                    StudioAutomationLevelV1::Balanced | StudioAutomationLevelV1::Autopilot => {
                        CreatorVisualActionV1::Generate
                    }
                };
                (routing, Vec::new(), None, None, Some(target), action)
            } else {
                let discovered = if stock_targets.is_empty() {
                    CreatorStockDiscoveryV1 {
                        status: StockDiscoveryStatusV1::Complete,
                        ranking_inputs: Vec::new(),
                    }
                } else {
                    discovery.discover_stock_v1(scene, &stock_targets)?
                };
                discovered.validate_v1(scene)?;
                let ranked_stock =
                    rank_visual_candidates(scene, discovered.ranking_inputs, ranking_policy)?;
                let routing = route_scene_visual_v1(
                    scene,
                    discovered.status,
                    &ranked_stock,
                    routing_policy,
                )?;

                match routing.route {
                    VisualRouteV1::StockReview => {
                        let review = build_visual_review(scene, &ranked_stock, review_options)?;
                        if review.candidates.is_empty() {
                            return Err(Error::InvalidContract(
                                "stock review route produced no review candidates".to_owned(),
                            ));
                        }
                        if studio_pack.config.automation_level == StudioAutomationLevelV1::Autopilot
                        {
                            let selected = review.recommended_candidate_id.clone().ok_or_else(|| {
                                Error::InvalidContract(
                                    "Autopilot stock review has no recommended candidate".to_owned(),
                                )
                            })?;
                            let candidate = ranked_stock
                                .iter()
                                .find(|ranked| ranked.candidate.candidate_id == selected)
                                .map(|ranked| &ranked.candidate)
                                .ok_or_else(|| {
                                    Error::InvalidContract(
                                        "Autopilot recommendation is outside ranked candidates"
                                            .to_owned(),
                                    )
                                })?;
                            let target = stock_target_for_candidate_v1(&route_targets, candidate)?;
                            (
                                routing,
                                ranked_stock,
                                Some(review),
                                Some(selected),
                                Some(target),
                                CreatorVisualActionV1::FetchSelectedStock,
                            )
                        } else {
                            (
                                routing,
                                ranked_stock,
                                Some(review),
                                None,
                                None,
                                CreatorVisualActionV1::AwaitingStockSelection,
                            )
                        }
                    }
                    VisualRouteV1::GeneratedStill => {
                        let target = generation_target_after_stock_v1(
                            &route_targets,
                            stock_prefix_end,
                        )?;
                        let action = match studio_pack.config.automation_level {
                            StudioAutomationLevelV1::Assisted => {
                                CreatorVisualActionV1::AwaitingGenerationApproval
                            }
                            StudioAutomationLevelV1::Balanced
                            | StudioAutomationLevelV1::Autopilot => {
                                CreatorVisualActionV1::Generate
                            }
                        };
                        (
                            routing,
                            ranked_stock,
                            None,
                            None,
                            Some(target),
                            action,
                        )
                    }
                }
            };

        let planned = CreatorVisualScenePlanV1 {
            scene_id: scene.id.clone(),
            route_key,
            route_targets,
            routing,
            ranked_stock,
            review,
            action,
            selected_candidate_id,
            execution_target,
        };
        planned.validate_v1()?;
        scenes.push(planned);
    }

    let plan = CreatorVisualPlanV1 {
        schema: CREATOR_VISUAL_PLAN_SCHEMA_V1.to_owned(),
        schema_version: CREATOR_VISUAL_PLAN_VERSION_V1,
        project_id: project.id.clone(),
        studio_pack_id: studio_pack.id.clone(),
        scene_plan_sha256: scene_plan_sha256.to_owned(),
        automation_level: studio_pack.config.automation_level,
        scenes,
    };
    plan.validate_v1()?;
    Ok(plan)
}

pub fn select_creator_stock_candidate_v1(
    plan: &mut CreatorVisualPlanV1,
    scene_id: &str,
    candidate_id: &str,
) -> Result<()> {
    plan.validate_v1()?;
    if candidate_id.trim().is_empty() {
        return Err(Error::InvalidContract(
            "creator visual candidate_id must not be empty".to_owned(),
        ));
    }
    let scene = plan
        .scenes
        .iter_mut()
        .find(|scene| scene.scene_id == scene_id)
        .ok_or_else(|| Error::InvalidContract(format!("visual scene not found: {scene_id}")))?;
    if scene.routing.route != VisualRouteV1::StockReview {
        return Err(Error::InvalidContract(
            "stock selection is only valid for stock-review routes".to_owned(),
        ));
    }
    let candidate = scene
        .ranked_stock
        .iter()
        .find(|ranked| ranked.candidate.candidate_id == candidate_id)
        .map(|ranked| ranked.candidate.clone())
        .ok_or_else(|| {
            Error::InvalidContract(format!(
                "candidate {candidate_id} is outside the ranked review set"
            ))
        })?;
    let target = stock_target_for_candidate_v1(&scene.route_targets, &candidate)?;
    scene.selected_candidate_id = Some(candidate_id.to_owned());
    scene.execution_target = Some(target);
    scene.action = CreatorVisualActionV1::FetchSelectedStock;
    scene.validate_v1()
}

pub fn approve_creator_generated_visual_v1(
    plan: &mut CreatorVisualPlanV1,
    scene_id: &str,
) -> Result<()> {
    plan.validate_v1()?;
    let scene = plan
        .scenes
        .iter_mut()
        .find(|scene| scene.scene_id == scene_id)
        .ok_or_else(|| Error::InvalidContract(format!("visual scene not found: {scene_id}")))?;
    if scene.action != CreatorVisualActionV1::AwaitingGenerationApproval {
        return Err(Error::InvalidContract(
            "generated approval is only valid while awaiting generation approval".to_owned(),
        ));
    }
    scene.action = CreatorVisualActionV1::Generate;
    scene.validate_v1()
}

pub fn visual_route_key_v1(scene: &SceneIntentV1) -> Result<String> {
    scene.validate_v1()?;
    match scene.scene_type.as_str() {
        "literal" | "emotional" | "conceptual" => Ok(format!("visual.{}", scene.scene_type)),
        other => Err(Error::InvalidContract(format!(
            "unsupported creator visual scene_type: {other}"
        ))),
    }
}

pub fn routing_policy_from_studio_pack_v1(
    studio_pack: &EffectiveStudioPackV1,
) -> Result<VisualRoutingPolicyV1> {
    studio_pack.validate_v1()?;
    let minimum_stock_score = studio_pack
        .config
        .quality_thresholds
        .get("visual")
        .map(|threshold| f64::from(*threshold) / 100.0)
        .unwrap_or(crate::DEFAULT_MINIMUM_STOCK_SCORE_V1);
    let policy = VisualRoutingPolicyV1 {
        minimum_stock_score,
    };
    policy.validate()?;
    Ok(policy)
}

fn generation_target_after_stock_v1(
    targets: &[StudioPackRouteTargetV1],
    start: usize,
) -> Result<StudioPackRouteTargetV1> {
    targets
        .iter()
        .skip(start)
        .find(|target| is_generation_target_v1(target))
        .cloned()
        .ok_or_else(|| {
            Error::InvalidContract(
                "stock fallback requested generation but Studio Pack has no generated/stick target"
                    .to_owned(),
            )
        })
}

fn stock_target_for_candidate_v1(
    targets: &[StudioPackRouteTargetV1],
    candidate: &VisualCandidate,
) -> Result<StudioPackRouteTargetV1> {
    candidate.validate()?;
    let capability = match candidate.media_type {
        VisualMediaType::Image => STOCK_IMAGE_CAPABILITY_V1,
        VisualMediaType::Video => STOCK_VIDEO_CAPABILITY_V1,
    };

    targets
        .iter()
        .find(|target| {
            target.plugin_type == "visual"
                && target.capability == capability
                && target.plugin_id.as_deref() == Some(candidate.source_provider.as_str())
        })
        .or_else(|| {
            targets.iter().find(|target| {
                target.plugin_type == "visual"
                    && target.capability == capability
                    && target.plugin_id.is_none()
            })
        })
        .cloned()
        .ok_or_else(|| {
            Error::InvalidContract(format!(
                "selected candidate provider {} is outside Studio Pack route policy for {}",
                candidate.source_provider, capability
            ))
        })
}

fn is_stock_target_v1(target: &StudioPackRouteTargetV1) -> bool {
    target.plugin_type == "visual"
        && matches!(
            target.capability.as_str(),
            STOCK_IMAGE_CAPABILITY_V1 | STOCK_VIDEO_CAPABILITY_V1
        )
}

fn is_generation_target_v1(target: &StudioPackRouteTargetV1) -> bool {
    target.plugin_type == "visual"
        && matches!(
            target.capability.as_str(),
            GENERATED_STILL_CAPABILITY_ROUTE_V1 | STICK_FIGURE_VISUAL_CAPABILITY_V1
        )
}

fn is_sha256_hex_v1(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use chrono::Utc;

    use super::*;
    use crate::{
        initial_studio_pack_catalog_v1, CreatorInputV1, SegmentV1, VisualCandidatePreview,
        VisualCandidateSignals, VisualPreviewKind, VoiceDirectionV1, CREATOR_CONTENT_SCHEMA_V1,
        CREATOR_CONTENT_VERSION_V1, CREATOR_SCENE_PLAN_SCHEMA_V1,
        CREATOR_SCENE_PLAN_VERSION_V1, SCENE_INTENT_SCHEMA, SCENE_INTENT_SCHEMA_VERSION,
        SEGMENT_SCHEMA, SEGMENT_SCHEMA_VERSION,
    };

    struct FixtureDiscovery {
        calls: Cell<usize>,
        inputs: Vec<VisualCandidateRankingInput>,
        status: StockDiscoveryStatusV1,
    }

    impl CreatorVisualDiscoveryExecutorV1 for FixtureDiscovery {
        fn discover_stock_v1(
            &self,
            _scene: &SceneIntentV1,
            _ordered_targets: &[StudioPackRouteTargetV1],
        ) -> Result<CreatorStockDiscoveryV1> {
            self.calls.set(self.calls.get() + 1);
            Ok(CreatorStockDiscoveryV1 {
                status: self.status,
                ranking_inputs: self.inputs.clone(),
            })
        }
    }

    fn project(pack_id: &str) -> Project {
        Project {
            id: "prj-visual".to_owned(),
            title: "Visual orchestration".to_owned(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            studio_pack: Some(pack_id.to_owned()),
            channel_profile: None,
            script_version: 1,
            production_lock: false,
        }
    }

    fn content() -> CreatorContentV1 {
        CreatorContentV1 {
            schema: CREATOR_CONTENT_SCHEMA_V1.to_owned(),
            schema_version: CREATOR_CONTENT_VERSION_V1,
            project_id: "prj-visual".to_owned(),
            source: CreatorInputV1::script("Rebuild trust carefully."),
            script: "Rebuild trust carefully.".to_owned(),
            segments: vec![SegmentV1 {
                schema: SEGMENT_SCHEMA.to_owned(),
                schema_version: SEGMENT_SCHEMA_VERSION,
                id: "S001".to_owned(),
                order: 1,
                text: "Rebuild trust carefully.".to_owned(),
                voice_direction: VoiceDirectionV1::default(),
            }],
        }
    }

    fn scene_plan(scene_type: &str) -> CreatorScenePlanV1 {
        CreatorScenePlanV1 {
            schema: CREATOR_SCENE_PLAN_SCHEMA_V1.to_owned(),
            schema_version: CREATOR_SCENE_PLAN_VERSION_V1,
            project_id: "prj-visual".to_owned(),
            content_sha256:
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            scenes: vec![SceneIntentV1 {
                schema: SCENE_INTENT_SCHEMA.to_owned(),
                schema_version: SCENE_INTENT_SCHEMA_VERSION,
                id: "SC001".to_owned(),
                segment_id: "S001".to_owned(),
                narration: "Rebuild trust carefully.".to_owned(),
                purpose: "Show patient rebuilding.".to_owned(),
                scene_type: scene_type.to_owned(),
                emotion_before: Some("uncertainty".to_owned()),
                emotion_after: Some("cautious hope".to_owned()),
                duration_hint: Some(8.0),
                visual_ideas: vec![
                    "hands repairing a bridge".to_owned(),
                    "careful woodworking".to_owned(),
                ],
                search_queries: vec![
                    "repair bridge careful hands".to_owned(),
                    "woodworking restoration morning".to_owned(),
                    "craftsperson rebuilding structure".to_owned(),
                ],
                avoid: vec!["generic praying hands".to_owned()],
                continuity: Default::default(),
                aspect_ratio: "16:9".to_owned(),
            }],
        }
    }

    fn ranked_input(score: f64) -> VisualCandidateRankingInput {
        VisualCandidateRankingInput {
            candidate: VisualCandidate {
                candidate_id: "pexels:video:42".to_owned(),
                scene_id: "SC001".to_owned(),
                source_provider: "pexels".to_owned(),
                source_asset_id: "42".to_owned(),
                selection_ref: "pexels:video:42".to_owned(),
                media_type: VisualMediaType::Video,
                title: Some("careful bridge repair".to_owned()),
                description: Some("workers restoring a wooden bridge".to_owned()),
                tags: vec!["repair".to_owned(), "bridge".to_owned()],
                source_page_url: Some("https://www.pexels.com/video/42/".to_owned()),
                creator_name: Some("Creator".to_owned()),
                creator_url: Some("https://www.pexels.com/@creator".to_owned()),
                width: Some(1920),
                height: Some(1080),
                duration: Some(8.0),
                previews: vec![VisualCandidatePreview {
                    kind: VisualPreviewKind::Thumbnail,
                    url: "https://images.pexels.com/photos/42/preview.jpg".to_owned(),
                    width: Some(640),
                    height: Some(360),
                    duration: None,
                }],
            },
            signals: VisualCandidateSignals {
                semantic_relevance: score,
                emotional_relevance: score,
                narrative_purpose: score,
                visual_quality: score,
                channel_continuity: score,
                editability: score,
                usage_count: 0,
                used_recently: false,
            },
        }
    }

    fn sha() -> &'static str {
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    }

    #[test]
    fn balanced_stock_route_is_preview_first_and_requires_explicit_selection() {
        let pack = initial_studio_pack_catalog_v1()
            .unwrap()
            .resolve_v1("christian-cinematic")
            .unwrap();
        let discovery = FixtureDiscovery {
            calls: Cell::new(0),
            inputs: vec![ranked_input(0.98)],
            status: StockDiscoveryStatusV1::Complete,
        };

        let mut plan = plan_creator_visuals_v1(
            &project(&pack.id),
            &pack,
            &content(),
            &scene_plan("literal"),
            sha(),
            &discovery,
            &VisualRankingPolicy::default(),
            VisualReviewOptions::default(),
        )
        .unwrap();

        assert_eq!(discovery.calls.get(), 1);
        assert_eq!(
            plan.scenes[0].action,
            CreatorVisualActionV1::AwaitingStockSelection
        );
        assert!(plan.scenes[0].routing.preview_first);
        assert!(plan.scenes[0].review.as_ref().unwrap().selection_required);
        assert!(plan.scenes[0].execution_target.is_none());

        select_creator_stock_candidate_v1(&mut plan, "SC001", "pexels:video:42").unwrap();
        assert_eq!(
            plan.scenes[0].action,
            CreatorVisualActionV1::FetchSelectedStock
        );
        assert_eq!(
            plan.scenes[0]
                .execution_target
                .as_ref()
                .and_then(|target| target.plugin_id.as_deref()),
            Some("pexels")
        );
    }

    #[test]
    fn Studio_Pack_generated_first_route_skips_stock_discovery() {
        let pack = initial_studio_pack_catalog_v1()
            .unwrap()
            .resolve_v1("christian-cinematic")
            .unwrap();
        let discovery = FixtureDiscovery {
            calls: Cell::new(0),
            inputs: vec![ranked_input(0.99)],
            status: StockDiscoveryStatusV1::Complete,
        };

        let plan = plan_creator_visuals_v1(
            &project(&pack.id),
            &pack,
            &content(),
            &scene_plan("conceptual"),
            sha(),
            &discovery,
            &VisualRankingPolicy::default(),
            VisualReviewOptions::default(),
        )
        .unwrap();

        assert_eq!(discovery.calls.get(), 0);
        assert_eq!(plan.scenes[0].action, CreatorVisualActionV1::Generate);
        assert_eq!(
            plan.scenes[0]
                .execution_target
                .as_ref()
                .unwrap()
                .capability,
            GENERATED_STILL_CAPABILITY_ROUTE_V1
        );
        assert_eq!(
            plan.scenes[0].routing.reason,
            crate::VisualRoutingReasonV1::StudioPackPreferredGenerated
        );
    }

    #[test]
    fn stick_explainer_routes_conceptual_scene_to_existing_stick_capability() {
        let pack = initial_studio_pack_catalog_v1()
            .unwrap()
            .resolve_v1("christian-stick-explainer")
            .unwrap();
        let discovery = FixtureDiscovery {
            calls: Cell::new(0),
            inputs: Vec::new(),
            status: StockDiscoveryStatusV1::Complete,
        };

        let plan = plan_creator_visuals_v1(
            &project(&pack.id),
            &pack,
            &content(),
            &scene_plan("conceptual"),
            sha(),
            &discovery,
            &VisualRankingPolicy::default(),
            VisualReviewOptions::default(),
        )
        .unwrap();

        assert_eq!(discovery.calls.get(), 0);
        assert_eq!(
            plan.scenes[0]
                .execution_target
                .as_ref()
                .unwrap()
                .capability,
            STICK_FIGURE_VISUAL_CAPABILITY_V1
        );
    }

    #[test]
    fn below_threshold_stock_falls_back_to_next_generated_target() {
        let pack = initial_studio_pack_catalog_v1()
            .unwrap()
            .resolve_v1("christian-cinematic")
            .unwrap();
        let discovery = FixtureDiscovery {
            calls: Cell::new(0),
            inputs: vec![ranked_input(0.20)],
            status: StockDiscoveryStatusV1::Complete,
        };

        let plan = plan_creator_visuals_v1(
            &project(&pack.id),
            &pack,
            &content(),
            &scene_plan("literal"),
            sha(),
            &discovery,
            &VisualRankingPolicy::default(),
            VisualReviewOptions::default(),
        )
        .unwrap();

        assert_eq!(plan.scenes[0].routing.route, VisualRouteV1::GeneratedStill);
        assert_eq!(
            plan.scenes[0]
                .execution_target
                .as_ref()
                .unwrap()
                .capability,
            GENERATED_STILL_CAPABILITY_ROUTE_V1
        );
    }

    #[test]
    fn assisted_generation_requires_approval_before_generate_action() {
        let catalog = initial_studio_pack_catalog_v1().unwrap();
        let mut pack = catalog.resolve_v1("christian-cinematic").unwrap();
        pack.config.automation_level = StudioAutomationLevelV1::Assisted;
        let discovery = FixtureDiscovery {
            calls: Cell::new(0),
            inputs: Vec::new(),
            status: StockDiscoveryStatusV1::Complete,
        };

        let mut plan = plan_creator_visuals_v1(
            &project(&pack.id),
            &pack,
            &content(),
            &scene_plan("conceptual"),
            sha(),
            &discovery,
            &VisualRankingPolicy::default(),
            VisualReviewOptions::default(),
        )
        .unwrap();

        assert_eq!(
            plan.scenes[0].action,
            CreatorVisualActionV1::AwaitingGenerationApproval
        );
        approve_creator_generated_visual_v1(&mut plan, "SC001").unwrap();
        assert_eq!(plan.scenes[0].action, CreatorVisualActionV1::Generate);
    }

    #[test]
    fn visual_quality_threshold_comes_from_resolved_Studio_Pack() {
        let pack = initial_studio_pack_catalog_v1()
            .unwrap()
            .resolve_v1("christian-cinematic")
            .unwrap();
        let policy = routing_policy_from_studio_pack_v1(&pack).unwrap();

        assert_eq!(policy.minimum_stock_score, 0.80);
    }
}
