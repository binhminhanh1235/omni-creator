use serde::{Deserialize, Serialize};

use crate::{
    visual_vision::VisualVisionEvaluationSet, Error, RankedVisualCandidate, Result, SceneIntentV1,
    VisualCandidate, VisualCandidatePreview, VisualCandidateScore, VisualMediaType,
    VisualPreviewKind,
};

pub const DEFAULT_VISUAL_REVIEW_LIMIT: usize = 3;
pub const MAX_VISUAL_REVIEW_LIMIT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualReviewOptions {
    pub max_candidates: usize,
}

impl Default for VisualReviewOptions {
    fn default() -> Self {
        Self {
            max_candidates: DEFAULT_VISUAL_REVIEW_LIMIT,
        }
    }
}

impl VisualReviewOptions {
    pub fn validate(&self) -> Result<()> {
        if !(1..=MAX_VISUAL_REVIEW_LIMIT).contains(&self.max_candidates) {
            return Err(Error::InvalidContract(format!(
                "visual review max_candidates must be between 1 and {MAX_VISUAL_REVIEW_LIMIT}"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisualReviewScoreBreakdown {
    pub semantic_points: u8,
    pub emotional_points: u8,
    pub purpose_points: u8,
    pub visual_quality_points: u8,
    pub continuity_points: u8,
    pub editability_points: u8,
    pub freshness_points: u8,
    pub penalty_points: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisualReviewVisionSummary {
    pub fit_percent: u8,
    pub semantic_points: u8,
    pub emotional_points: u8,
    pub purpose_points: u8,
    pub visual_quality_points: u8,
    pub editability_points: u8,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualReviewCandidate {
    pub rank: u32,
    pub candidate_id: String,
    pub media_type: VisualMediaType,
    pub title: Option<String>,
    pub creator_name: Option<String>,
    pub preview: VisualCandidatePreview,
    pub score_percent: u8,
    pub score_breakdown: VisualReviewScoreBreakdown,
    pub rationale: String,
    #[serde(default)]
    pub cautions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision: Option<VisualReviewVisionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualReviewSet {
    pub scene_id: String,
    pub narration: String,
    pub purpose: String,
    pub scene_type: String,
    pub recommended_candidate_id: Option<String>,
    pub selection_required: bool,
    pub candidates: Vec<VisualReviewCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualReviewAdvancedDetails {
    pub candidate: VisualCandidate,
    pub score: VisualCandidateScore,
}

/// Builds the default human-in-the-loop review payload.
///
/// This deliberately omits source asset IDs, selection refs, source URLs, dimensions, duration,
/// and provider-specific file details. Core keeps the complete ranked candidates separately and
/// resolves the selected candidate by candidate_id.
pub fn build_visual_review(
    scene: &SceneIntentV1,
    ranked: &[RankedVisualCandidate],
    options: VisualReviewOptions,
) -> Result<VisualReviewSet> {
    scene.validate_v1()?;
    options.validate()?;

    let mut candidates = Vec::new();
    for (index, ranked_candidate) in ranked.iter().take(options.max_candidates).enumerate() {
        ranked_candidate.candidate.validate()?;
        if ranked_candidate.candidate.scene_id != scene.id {
            return Err(Error::InvalidContract(format!(
                "visual review candidate {} belongs to scene {}, expected {}",
                ranked_candidate.candidate.candidate_id,
                ranked_candidate.candidate.scene_id,
                scene.id
            )));
        }

        let preview = choose_review_preview(&ranked_candidate.candidate).ok_or_else(|| {
            Error::InvalidContract(format!(
                "visual review candidate {} has no usable preview",
                ranked_candidate.candidate.candidate_id
            ))
        })?;

        candidates.push(VisualReviewCandidate {
            rank: (index + 1) as u32,
            candidate_id: ranked_candidate.candidate.candidate_id.clone(),
            media_type: ranked_candidate.candidate.media_type,
            title: ranked_candidate.candidate.title.clone(),
            creator_name: ranked_candidate.candidate.creator_name.clone(),
            preview,
            score_percent: percentage(ranked_candidate.score.final_score),
            score_breakdown: score_breakdown(&ranked_candidate.score),
            rationale: deterministic_rationale(&ranked_candidate.score),
            cautions: score_cautions(&ranked_candidate.score),
            vision: None,
        });
    }

    Ok(VisualReviewSet {
        scene_id: scene.id.clone(),
        narration: scene.narration.clone(),
        purpose: scene.purpose.clone(),
        scene_type: scene.scene_type.clone(),
        recommended_candidate_id: candidates
            .first()
            .map(|candidate| candidate.candidate_id.clone()),
        selection_required: true,
        candidates,
    })
}

pub fn enrich_visual_review_with_vision(
    review: &mut VisualReviewSet,
    vision: &VisualVisionEvaluationSet,
) -> Result<usize> {
    let mut enriched = 0usize;

    for evaluation in &vision.evaluations {
        evaluation.validate()?;
        let candidate = review
            .candidates
            .iter_mut()
            .find(|candidate| candidate.candidate_id == evaluation.candidate_id)
            .ok_or_else(|| {
                Error::InvalidContract(format!(
                    "visual vision evaluation references candidate outside review set: {}",
                    evaluation.candidate_id
                ))
            })?;

        candidate.vision = Some(VisualReviewVisionSummary {
            fit_percent: percentage(evaluation.fit_score()),
            semantic_points: percentage(evaluation.semantic_relevance),
            emotional_points: percentage(evaluation.emotional_relevance),
            purpose_points: percentage(evaluation.narrative_purpose),
            visual_quality_points: percentage(evaluation.visual_quality),
            editability_points: percentage(evaluation.editability),
            rationale: evaluation.rationale.trim().to_owned(),
        });
        enriched += 1;
    }

    Ok(enriched)
}

pub fn visual_review_advanced_details(
    ranked: &[RankedVisualCandidate],
    candidate_id: &str,
) -> Result<VisualReviewAdvancedDetails> {
    if candidate_id.trim().is_empty() {
        return Err(Error::InvalidContract(
            "visual review candidate_id must not be empty".to_owned(),
        ));
    }

    let ranked_candidate = ranked
        .iter()
        .find(|candidate| candidate.candidate.candidate_id == candidate_id)
        .ok_or_else(|| {
            Error::InvalidContract(format!(
                "visual review candidate was not found: {candidate_id}"
            ))
        })?;

    Ok(VisualReviewAdvancedDetails {
        candidate: ranked_candidate.candidate.clone(),
        score: ranked_candidate.score.clone(),
    })
}

fn choose_review_preview(candidate: &VisualCandidate) -> Option<VisualCandidatePreview> {
    for kind in [
        VisualPreviewKind::Thumbnail,
        VisualPreviewKind::Image,
        VisualPreviewKind::Video,
    ] {
        if let Some(preview) = candidate.previews.iter().find(|preview| preview.kind == kind) {
            return Some(preview.clone());
        }
    }
    None
}

fn score_breakdown(score: &VisualCandidateScore) -> VisualReviewScoreBreakdown {
    VisualReviewScoreBreakdown {
        semantic_points: percentage(score.semantic_relevance),
        emotional_points: percentage(score.emotional_relevance),
        purpose_points: percentage(score.narrative_purpose),
        visual_quality_points: percentage(score.visual_quality),
        continuity_points: percentage(score.channel_continuity),
        editability_points: percentage(score.editability),
        freshness_points: percentage(score.freshness),
        penalty_points: percentage(score.cliche_penalty + score.reuse_penalty),
    }
}

fn deterministic_rationale(score: &VisualCandidateScore) -> String {
    let mut strengths = [
        ("meaning match", score.semantic_relevance),
        ("emotional fit", score.emotional_relevance),
        ("narrative purpose", score.narrative_purpose),
        ("visual quality", score.visual_quality),
        ("channel continuity", score.channel_continuity),
        ("editability", score.editability),
        ("freshness", score.freshness),
    ];
    strengths.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(right.0))
    });

    let first = strengths[0].0;
    let second = strengths[1].0;
    format!(
        "Ranks highly for {first} and {second}; overall fit is {}%.",
        percentage(score.final_score)
    )
}

fn score_cautions(score: &VisualCandidateScore) -> Vec<String> {
    let mut cautions = Vec::new();

    if !score.cliche_matches.is_empty() {
        cautions.push(format!(
            "Cliché overlap: {}.",
            score.cliche_matches.join(", ")
        ));
    }
    if score.reuse_penalty > 0.0 {
        cautions.push("Recently or repeatedly used visual.".to_owned());
    }

    cautions
}

fn percentage(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 100.0).round() as u8
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        RankedVisualCandidate, VisualCandidate, VisualCandidateScore, VisualMediaType,
        VisualPreviewKind, SCENE_INTENT_SCHEMA, SCENE_INTENT_SCHEMA_VERSION,
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

    fn ranked(id: &str, final_score: f64, cliche: bool) -> RankedVisualCandidate {
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
                creator_url: Some("https://www.pexels.com/@fixture".to_owned()),
                width: Some(3840),
                height: Some(2160),
                duration: Some(12.0),
                previews: vec![
                    VisualCandidatePreview {
                        kind: VisualPreviewKind::Video,
                        url: format!("https://preview.example/{id}.mp4"),
                        width: None,
                        height: None,
                        duration: Some(4.0),
                    },
                    VisualCandidatePreview {
                        kind: VisualPreviewKind::Thumbnail,
                        url: format!("https://preview.example/{id}.jpg"),
                        width: Some(640),
                        height: Some(360),
                        duration: None,
                    },
                ],
            },
            score: VisualCandidateScore {
                semantic_relevance: 0.31,
                emotional_relevance: 0.16,
                narrative_purpose: 0.12,
                visual_quality: 0.08,
                channel_continuity: 0.07,
                editability: 0.04,
                freshness: 0.05,
                content_match_score: 0.59,
                base_score: 0.83,
                cliche_matches: if cliche {
                    vec!["open bible".to_owned()]
                } else {
                    Vec::new()
                },
                cliche_penalty: if cliche { 0.08 } else { 0.0 },
                reuse_penalty: 0.0,
                final_score,
            },
        }
    }

    #[test]
    fn default_review_returns_only_top_three_and_requires_selection() {
        let ranked = vec![
            ranked("A", 0.92, false),
            ranked("B", 0.88, false),
            ranked("C", 0.81, false),
            ranked("D", 0.79, false),
        ];

        let review =
            build_visual_review(&scene(), &ranked, VisualReviewOptions::default()).unwrap();

        assert_eq!(review.candidates.len(), 3);
        assert_eq!(review.recommended_candidate_id.as_deref(), Some("A"));
        assert!(review.selection_required);
        assert_eq!(
            review
                .candidates
                .iter()
                .map(|candidate| candidate.candidate_id.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "B", "C"]
        );
    }

    #[test]
    fn default_review_payload_omits_provider_and_download_details() {
        let review = build_visual_review(
            &scene(),
            &[ranked("A", 0.92, false)],
            VisualReviewOptions::default(),
        )
        .unwrap();
        let value = serde_json::to_value(&review).unwrap();
        let candidate = &value["candidates"][0];

        assert!(candidate.get("candidate_id").is_some());
        assert!(candidate.get("preview").is_some());
        assert!(candidate.get("score_breakdown").is_some());
        assert!(candidate.get("rationale").is_some());

        assert!(candidate.get("source_provider").is_none());
        assert!(candidate.get("source_asset_id").is_none());
        assert!(candidate.get("selection_ref").is_none());
        assert!(candidate.get("source_page_url").is_none());
        assert!(candidate.get("width").is_none());
        assert!(candidate.get("height").is_none());
        assert!(candidate.get("duration").is_none());
        assert!(candidate.get("vision").is_none());
    }

    #[test]
    fn review_prefers_thumbnail_over_video_preview() {
        let review = build_visual_review(
            &scene(),
            &[ranked("A", 0.92, false)],
            VisualReviewOptions::default(),
        )
        .unwrap();

        assert_eq!(
            review.candidates[0].preview.kind,
            VisualPreviewKind::Thumbnail
        );
        assert!(review.candidates[0].preview.url.ends_with(".jpg"));
    }

    #[test]
    fn score_breakdown_and_cliche_caution_are_user_facing() {
        let review = build_visual_review(
            &scene(),
            &[ranked("A", 0.75, true)],
            VisualReviewOptions::default(),
        )
        .unwrap();
        let candidate = &review.candidates[0];

        assert_eq!(candidate.score_percent, 75);
        assert_eq!(candidate.score_breakdown.semantic_points, 31);
        assert_eq!(candidate.score_breakdown.penalty_points, 8);
        assert!(candidate.rationale.contains("overall fit is 75%"));
        assert_eq!(candidate.cautions, vec!["Cliché overlap: open bible."]);
    }

    #[test]
    fn advanced_details_keep_full_candidate_and_selection_ref_outside_default_payload() {
        let ranked = vec![ranked("A", 0.92, false)];
        let details = visual_review_advanced_details(&ranked, "A").unwrap();

        assert_eq!(details.candidate.source_provider, "pexels");
        assert_eq!(details.candidate.selection_ref, "pexels:video:A");
        assert_eq!(details.candidate.width, Some(3840));
        assert_eq!(details.score.final_score, 0.92);
    }

    #[test]
    fn vision_enrichment_adds_human_facing_summary_without_changing_rank() {
        let mut review = build_visual_review(
            &scene(),
            &[ranked("A", 0.92, false), ranked("B", 0.88, false)],
            VisualReviewOptions::default(),
        )
        .unwrap();

        let before = review
            .candidates
            .iter()
            .map(|candidate| candidate.candidate_id.clone())
            .collect::<Vec<_>>();

        let count = enrich_visual_review_with_vision(
            &mut review,
            &VisualVisionEvaluationSet {
                evaluations: vec![crate::VisualVisionEvaluation {
                    candidate_id: "A".to_owned(),
                    semantic_relevance: 0.9,
                    emotional_relevance: 0.8,
                    narrative_purpose: 0.7,
                    visual_quality: 0.8,
                    editability: 0.6,
                    rationale: "The preview directly supports cautious rebuilding.".to_owned(),
                }],
            },
        )
        .unwrap();

        assert_eq!(count, 1);
        assert_eq!(
            before,
            review
                .candidates
                .iter()
                .map(|candidate| candidate.candidate_id.clone())
                .collect::<Vec<_>>()
        );
        let summary = review.candidates[0].vision.as_ref().unwrap();
        assert_eq!(summary.fit_percent, 76);
        assert_eq!(summary.semantic_points, 90);
        assert!(summary.rationale.contains("cautious rebuilding"));
        assert!(review.candidates[1].vision.is_none());
    }

    #[test]
    fn review_rejects_candidate_for_other_scene() {
        let mut wrong = ranked("A", 0.92, false);
        wrong.candidate.scene_id = "SC99".to_owned();

        let error =
            build_visual_review(&scene(), &[wrong], VisualReviewOptions::default()).unwrap_err();

        assert!(error.to_string().contains("expected SC17"));
    }

    #[test]
    fn review_limit_is_bounded() {
        let error = build_visual_review(
            &scene(),
            &[],
            VisualReviewOptions {
                max_candidates: MAX_VISUAL_REVIEW_LIMIT + 1,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("max_candidates"));
    }
}
