use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    artifact_store::PluginOutputPromotion, Error, RankedVisualCandidate, Result, SceneIntentV1,
};

pub const VISUAL_ROUTING_DECISION_SCHEMA_V1: &str = "omnicreator.visual-routing-decision";
pub const DEFAULT_MINIMUM_STOCK_SCORE_V1: f64 = 0.80;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisualUseCaseV1 {
    SceneVisual,
    ThumbnailBackground,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisualRouteV1 {
    StockReview,
    GeneratedStill,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisualRoutingReasonV1 {
    StockMeetsQualityThreshold,
    NoStockCandidates,
    StockBelowQualityThreshold,
    StockUnavailable,
    ThumbnailBackgroundRequested,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StockDiscoveryStatusV1 {
    Complete,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct VisualRoutingPolicyV1 {
    pub minimum_stock_score: f64,
}

impl Default for VisualRoutingPolicyV1 {
    fn default() -> Self {
        Self {
            minimum_stock_score: DEFAULT_MINIMUM_STOCK_SCORE_V1,
        }
    }
}

impl VisualRoutingPolicyV1 {
    pub fn validate(&self) -> Result<()> {
        if !self.minimum_stock_score.is_finite() || !(0.0..=1.0).contains(&self.minimum_stock_score)
        {
            return Err(Error::InvalidContract(
                "visual routing minimum_stock_score must be finite and within 0.0..=1.0".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualRoutingDecisionV1 {
    pub schema: String,
    pub version: u32,
    pub scene_id: String,
    pub use_case: VisualUseCaseV1,
    pub route: VisualRouteV1,
    pub reason: VisualRoutingReasonV1,
    pub stock_candidate_id: Option<String>,
    pub stock_score: Option<f64>,
    pub minimum_stock_score: Option<f64>,
    pub preview_first: bool,
}

impl VisualRoutingDecisionV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != VISUAL_ROUTING_DECISION_SCHEMA_V1 || self.version != 1 {
            return Err(Error::InvalidContract(
                "unsupported visual routing decision schema/version".to_owned(),
            ));
        }
        if self.scene_id.trim().is_empty() {
            return Err(Error::InvalidContract(
                "visual routing scene_id must not be empty".to_owned(),
            ));
        }
        if let Some(candidate_id) = self.stock_candidate_id.as_deref() {
            if candidate_id.trim().is_empty() {
                return Err(Error::InvalidContract(
                    "visual routing stock_candidate_id must not be blank".to_owned(),
                ));
            }
        }
        if let Some(score) = self.stock_score {
            if !score.is_finite() || !(0.0..=1.0).contains(&score) {
                return Err(Error::InvalidContract(
                    "visual routing stock_score must be finite and within 0.0..=1.0".to_owned(),
                ));
            }
        }
        if let Some(threshold) = self.minimum_stock_score {
            if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
                return Err(Error::InvalidContract(
                    "visual routing minimum_stock_score must be finite and within 0.0..=1.0"
                        .to_owned(),
                ));
            }
        }

        match (self.use_case, self.route, self.reason) {
            (
                VisualUseCaseV1::SceneVisual,
                VisualRouteV1::StockReview,
                VisualRoutingReasonV1::StockMeetsQualityThreshold,
            ) => {
                if self.stock_candidate_id.is_none()
                    || self.stock_score.is_none()
                    || self.minimum_stock_score.is_none()
                    || !self.preview_first
                {
                    return Err(Error::InvalidContract(
                        "stock review routing requires candidate, score, threshold, and preview_first"
                            .to_owned(),
                    ));
                }
            }
            (
                VisualUseCaseV1::SceneVisual,
                VisualRouteV1::GeneratedStill,
                VisualRoutingReasonV1::NoStockCandidates | VisualRoutingReasonV1::StockUnavailable,
            ) => {
                if self.stock_candidate_id.is_some()
                    || self.stock_score.is_some()
                    || self.minimum_stock_score.is_none()
                    || self.preview_first
                {
                    return Err(Error::InvalidContract(
                        "empty/unavailable stock fallback has inconsistent routing metadata"
                            .to_owned(),
                    ));
                }
            }
            (
                VisualUseCaseV1::SceneVisual,
                VisualRouteV1::GeneratedStill,
                VisualRoutingReasonV1::StockBelowQualityThreshold,
            ) => {
                if self.stock_candidate_id.is_none()
                    || self.stock_score.is_none()
                    || self.minimum_stock_score.is_none()
                    || self.preview_first
                {
                    return Err(Error::InvalidContract(
                        "below-threshold stock fallback requires rejected candidate score metadata"
                            .to_owned(),
                    ));
                }
            }
            (
                VisualUseCaseV1::ThumbnailBackground,
                VisualRouteV1::GeneratedStill,
                VisualRoutingReasonV1::ThumbnailBackgroundRequested,
            ) => {
                if self.stock_candidate_id.is_some()
                    || self.stock_score.is_some()
                    || self.minimum_stock_score.is_some()
                    || self.preview_first
                {
                    return Err(Error::InvalidContract(
                        "thumbnail background routing must not contain stock decision fields"
                            .to_owned(),
                    ));
                }
            }
            _ => {
                return Err(Error::InvalidContract(
                    "visual routing decision contains an invalid use_case/route/reason combination"
                        .to_owned(),
                ));
            }
        }

        Ok(())
    }

    pub fn is_generated(&self) -> bool {
        self.route == VisualRouteV1::GeneratedStill
    }
}

#[derive(Debug, Clone)]
struct VisualRoutingStockFieldsV1 {
    candidate_id: Option<String>,
    score: Option<f64>,
    minimum_score: Option<f64>,
    preview_first: bool,
}

impl VisualRoutingStockFieldsV1 {
    fn unavailable_or_empty(minimum_score: f64) -> Self {
        Self {
            candidate_id: None,
            score: None,
            minimum_score: Some(minimum_score),
            preview_first: false,
        }
    }

    fn candidate(
        candidate_id: String,
        score: f64,
        minimum_score: f64,
        preview_first: bool,
    ) -> Self {
        Self {
            candidate_id: Some(candidate_id),
            score: Some(score),
            minimum_score: Some(minimum_score),
            preview_first,
        }
    }

    fn none() -> Self {
        Self {
            candidate_id: None,
            score: None,
            minimum_score: None,
            preview_first: false,
        }
    }
}

pub fn route_scene_visual_v1(
    scene: &SceneIntentV1,
    stock_status: StockDiscoveryStatusV1,
    ranked_stock: &[RankedVisualCandidate],
    policy: VisualRoutingPolicyV1,
) -> Result<VisualRoutingDecisionV1> {
    scene.validate_v1()?;
    policy.validate()?;

    for ranked in ranked_stock {
        ranked.candidate.validate()?;
        if ranked.candidate.scene_id != scene.id {
            return Err(Error::InvalidContract(format!(
                "visual routing candidate {} belongs to scene {}, expected {}",
                ranked.candidate.candidate_id, ranked.candidate.scene_id, scene.id
            )));
        }
        if !ranked.score.final_score.is_finite() || !(0.0..=1.0).contains(&ranked.score.final_score)
        {
            return Err(Error::InvalidContract(format!(
                "visual routing candidate {} has invalid final_score",
                ranked.candidate.candidate_id
            )));
        }
    }

    if stock_status == StockDiscoveryStatusV1::Unavailable {
        if !ranked_stock.is_empty() {
            return Err(Error::InvalidContract(
                "stock discovery marked unavailable must not include ranked candidates".to_owned(),
            ));
        }
        return decision(
            scene,
            VisualUseCaseV1::SceneVisual,
            VisualRouteV1::GeneratedStill,
            VisualRoutingReasonV1::StockUnavailable,
            VisualRoutingStockFieldsV1::unavailable_or_empty(policy.minimum_stock_score),
        );
    }

    let best = ranked_stock.iter().max_by(|left, right| {
        left.score
            .final_score
            .total_cmp(&right.score.final_score)
            .then_with(|| {
                right
                    .candidate
                    .candidate_id
                    .cmp(&left.candidate.candidate_id)
            })
    });

    let Some(best) = best else {
        return decision(
            scene,
            VisualUseCaseV1::SceneVisual,
            VisualRouteV1::GeneratedStill,
            VisualRoutingReasonV1::NoStockCandidates,
            VisualRoutingStockFieldsV1::unavailable_or_empty(policy.minimum_stock_score),
        );
    };

    if best.score.final_score >= policy.minimum_stock_score {
        decision(
            scene,
            VisualUseCaseV1::SceneVisual,
            VisualRouteV1::StockReview,
            VisualRoutingReasonV1::StockMeetsQualityThreshold,
            VisualRoutingStockFieldsV1::candidate(
                best.candidate.candidate_id.clone(),
                best.score.final_score,
                policy.minimum_stock_score,
                true,
            ),
        )
    } else {
        decision(
            scene,
            VisualUseCaseV1::SceneVisual,
            VisualRouteV1::GeneratedStill,
            VisualRoutingReasonV1::StockBelowQualityThreshold,
            VisualRoutingStockFieldsV1::candidate(
                best.candidate.candidate_id.clone(),
                best.score.final_score,
                policy.minimum_stock_score,
                false,
            ),
        )
    }
}

pub fn route_thumbnail_background_v1(scene: &SceneIntentV1) -> Result<VisualRoutingDecisionV1> {
    scene.validate_v1()?;
    decision(
        scene,
        VisualUseCaseV1::ThumbnailBackground,
        VisualRouteV1::GeneratedStill,
        VisualRoutingReasonV1::ThumbnailBackgroundRequested,
        VisualRoutingStockFieldsV1::none(),
    )
}

pub fn attach_visual_routing_to_promotion_v1(
    mut promotion: PluginOutputPromotion,
    routing: &VisualRoutingDecisionV1,
) -> Result<PluginOutputPromotion> {
    routing.validate_v1()?;
    let object = promotion.metadata.as_object_mut().ok_or_else(|| {
        Error::InvalidArtifact(
            "visual routing provenance requires object-shaped artifact metadata".to_owned(),
        )
    })?;
    object.insert("visual_routing".to_owned(), serde_json::to_value(routing)?);
    Ok(promotion)
}

fn decision(
    scene: &SceneIntentV1,
    use_case: VisualUseCaseV1,
    route: VisualRouteV1,
    reason: VisualRoutingReasonV1,
    stock: VisualRoutingStockFieldsV1,
) -> Result<VisualRoutingDecisionV1> {
    let decision = VisualRoutingDecisionV1 {
        schema: VISUAL_ROUTING_DECISION_SCHEMA_V1.to_owned(),
        version: 1,
        scene_id: scene.id.clone(),
        use_case,
        route,
        reason,
        stock_candidate_id: stock.candidate_id,
        stock_score: stock.score,
        minimum_stock_score: stock.minimum_score,
        preview_first: stock.preview_first,
    };
    decision.validate_v1()?;
    Ok(decision)
}

pub fn visual_routing_provenance_value_v1(routing: &VisualRoutingDecisionV1) -> Result<Value> {
    routing.validate_v1()?;
    Ok(serde_json::to_value(routing)?)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        RankedVisualCandidate, VisualCandidate, VisualCandidatePreview, VisualCandidateScore,
        VisualMediaType, VisualPreviewKind, SCENE_INTENT_SCHEMA, SCENE_INTENT_SCHEMA_VERSION,
    };

    use super::*;

    fn scene() -> SceneIntentV1 {
        SceneIntentV1 {
            schema: SCENE_INTENT_SCHEMA.to_owned(),
            schema_version: SCENE_INTENT_SCHEMA_VERSION,
            id: "SC17".to_owned(),
            segment_id: "S04".to_owned(),
            narration: "Forgiveness does not automatically restore trust.".to_owned(),
            purpose: "Show cautious rebuilding of trust.".to_owned(),
            scene_type: "conceptual".to_owned(),
            emotion_before: Some("uncertainty".to_owned()),
            emotion_after: Some("cautious hope".to_owned()),
            duration_hint: Some(11.5),
            visual_ideas: vec!["repairing a bridge".to_owned()],
            search_queries: vec!["rebuilding fence careful hands".to_owned()],
            avoid: vec!["generic praying hands".to_owned()],
            continuity: BTreeMap::new(),
            aspect_ratio: "16:9".to_owned(),
        }
    }

    fn ranked(id: &str, score: f64) -> RankedVisualCandidate {
        RankedVisualCandidate {
            candidate: VisualCandidate {
                candidate_id: id.to_owned(),
                scene_id: "SC17".to_owned(),
                source_provider: "pexels".to_owned(),
                source_asset_id: format!("asset-{id}"),
                selection_ref: format!("pexels:video:{id}"),
                media_type: VisualMediaType::Video,
                title: Some(format!("Candidate {id}")),
                description: None,
                tags: Vec::new(),
                source_page_url: Some(format!("https://www.pexels.com/video/{id}/")),
                creator_name: Some("Fixture Creator".to_owned()),
                creator_url: None,
                width: Some(1920),
                height: Some(1080),
                duration: Some(9.0),
                previews: vec![VisualCandidatePreview {
                    kind: VisualPreviewKind::Thumbnail,
                    url: format!("https://preview.example/{id}.jpg"),
                    width: Some(640),
                    height: Some(360),
                    duration: None,
                }],
            },
            score: VisualCandidateScore {
                semantic_relevance: 0.3,
                emotional_relevance: 0.15,
                narrative_purpose: 0.12,
                visual_quality: 0.08,
                channel_continuity: 0.07,
                editability: 0.04,
                freshness: 0.04,
                content_match_score: 0.57,
                base_score: score,
                cliche_matches: Vec::new(),
                cliche_penalty: 0.0,
                reuse_penalty: 0.0,
                final_score: score,
            },
        }
    }

    #[test]
    fn viable_stock_routes_to_preview_first_review() {
        let routing = route_scene_visual_v1(
            &scene(),
            StockDiscoveryStatusV1::Complete,
            &[ranked("B", 0.83), ranked("A", 0.90)],
            VisualRoutingPolicyV1 {
                minimum_stock_score: 0.85,
            },
        )
        .unwrap();

        assert_eq!(routing.route, VisualRouteV1::StockReview);
        assert_eq!(
            routing.reason,
            VisualRoutingReasonV1::StockMeetsQualityThreshold
        );
        assert_eq!(routing.stock_candidate_id.as_deref(), Some("A"));
        assert_eq!(routing.stock_score, Some(0.90));
        assert!(routing.preview_first);

        let encoded = serde_json::to_string(&routing).unwrap();
        assert!(!encoded.contains("pexels"));
        assert!(!encoded.contains("selection_ref"));
        assert!(!encoded.contains("source_asset_id"));
    }

    #[test]
    fn low_quality_stock_routes_to_generated_still() {
        let routing = route_scene_visual_v1(
            &scene(),
            StockDiscoveryStatusV1::Complete,
            &[ranked("A", 0.72)],
            VisualRoutingPolicyV1 {
                minimum_stock_score: 0.80,
            },
        )
        .unwrap();

        assert_eq!(routing.route, VisualRouteV1::GeneratedStill);
        assert_eq!(
            routing.reason,
            VisualRoutingReasonV1::StockBelowQualityThreshold
        );
        assert_eq!(routing.stock_candidate_id.as_deref(), Some("A"));
        assert_eq!(routing.stock_score, Some(0.72));
        assert!(!routing.preview_first);
    }

    #[test]
    fn no_stock_and_unavailable_stock_have_stable_distinct_reasons() {
        let no_stock = route_scene_visual_v1(
            &scene(),
            StockDiscoveryStatusV1::Complete,
            &[],
            VisualRoutingPolicyV1::default(),
        )
        .unwrap();
        let unavailable = route_scene_visual_v1(
            &scene(),
            StockDiscoveryStatusV1::Unavailable,
            &[],
            VisualRoutingPolicyV1::default(),
        )
        .unwrap();

        assert_eq!(no_stock.reason, VisualRoutingReasonV1::NoStockCandidates);
        assert_eq!(unavailable.reason, VisualRoutingReasonV1::StockUnavailable);
        assert!(no_stock.is_generated());
        assert!(unavailable.is_generated());
    }

    #[test]
    fn thumbnail_background_is_generated_without_stock_state() {
        let routing = route_thumbnail_background_v1(&scene()).unwrap();

        assert_eq!(routing.use_case, VisualUseCaseV1::ThumbnailBackground);
        assert_eq!(routing.route, VisualRouteV1::GeneratedStill);
        assert_eq!(
            routing.reason,
            VisualRoutingReasonV1::ThumbnailBackgroundRequested
        );
        assert_eq!(routing.minimum_stock_score, None);
        assert_eq!(routing.stock_candidate_id, None);
    }

    #[test]
    fn deterministic_tie_break_uses_candidate_identity() {
        let routing = route_scene_visual_v1(
            &scene(),
            StockDiscoveryStatusV1::Complete,
            &[ranked("B", 0.90), ranked("A", 0.90)],
            VisualRoutingPolicyV1 {
                minimum_stock_score: 0.85,
            },
        )
        .unwrap();

        assert_eq!(routing.stock_candidate_id.as_deref(), Some("A"));
    }

    #[test]
    fn unavailable_stock_rejects_contradictory_candidates() {
        let error = route_scene_visual_v1(
            &scene(),
            StockDiscoveryStatusV1::Unavailable,
            &[ranked("A", 0.90)],
            VisualRoutingPolicyV1::default(),
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("must not include ranked candidates"));
    }
}
