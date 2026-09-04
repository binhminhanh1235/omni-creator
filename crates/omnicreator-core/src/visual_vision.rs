use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    llmgateway::decode_structured_output, Error, LlmGatewayClient, RankedVisualCandidate, Result,
    SceneIntentV1, VisualCandidate, VisualCandidatePreview, VisualPreviewKind,
};

pub const VISUAL_VISION_CONTRACT: &str = "omnicreator.visual-vision-evaluation.v1";
pub const DEFAULT_VISUAL_VISION_LIMIT: usize = 3;
pub const MAX_VISUAL_VISION_LIMIT: usize = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct VisualVisionOptions {
    pub model: Option<String>,
    pub max_candidates: usize,
    pub max_tokens: u32,
}

impl Default for VisualVisionOptions {
    fn default() -> Self {
        Self {
            model: None,
            max_candidates: DEFAULT_VISUAL_VISION_LIMIT,
            max_tokens: 1400,
        }
    }
}

impl VisualVisionOptions {
    pub fn validate(&self) -> Result<()> {
        if self
            .model
            .as_ref()
            .is_some_and(|model| model.trim().is_empty())
        {
            return Err(Error::InvalidLlmGatewayConfig(
                "visual vision model must not be empty when present".to_owned(),
            ));
        }
        if !(1..=MAX_VISUAL_VISION_LIMIT).contains(&self.max_candidates) {
            return Err(Error::InvalidContract(format!(
                "visual vision max_candidates must be between 1 and {MAX_VISUAL_VISION_LIMIT}"
            )));
        }
        if !(256..=8192).contains(&self.max_tokens) {
            return Err(Error::InvalidLlmGatewayConfig(
                "visual vision max_tokens must be between 256 and 8192".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualVisionEvaluation {
    pub candidate_id: String,
    pub semantic_relevance: f64,
    pub emotional_relevance: f64,
    pub narrative_purpose: f64,
    pub visual_quality: f64,
    pub editability: f64,
    pub rationale: String,
}

impl VisualVisionEvaluation {
    pub fn validate(&self) -> Result<()> {
        if self.candidate_id.trim().is_empty() {
            return Err(Error::InvalidContract(
                "visual vision candidate_id must not be empty".to_owned(),
            ));
        }
        for (label, value) in [
            ("semantic_relevance", self.semantic_relevance),
            ("emotional_relevance", self.emotional_relevance),
            ("narrative_purpose", self.narrative_purpose),
            ("visual_quality", self.visual_quality),
            ("editability", self.editability),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(Error::InvalidContract(format!(
                    "visual vision {label} must be finite and between 0.0 and 1.0"
                )));
            }
        }
        if self.rationale.trim().is_empty() {
            return Err(Error::InvalidContract(
                "visual vision rationale must not be empty".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn fit_score(&self) -> f64 {
        (self.semantic_relevance
            + self.emotional_relevance
            + self.narrative_purpose
            + self.visual_quality
            + self.editability)
            / 5.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualVisionEvaluationSet {
    pub evaluations: Vec<VisualVisionEvaluation>,
}

impl VisualVisionEvaluationSet {
    pub fn validate_for_candidates(&self, candidate_ids: &[String]) -> Result<()> {
        let expected = candidate_ids.iter().cloned().collect::<BTreeSet<_>>();
        let mut found = BTreeSet::new();

        if self.evaluations.len() != expected.len() {
            return Err(Error::InvalidContract(format!(
                "visual vision returned {} evaluations for {} requested candidates",
                self.evaluations.len(),
                expected.len()
            )));
        }

        for evaluation in &self.evaluations {
            evaluation.validate()?;
            if !expected.contains(&evaluation.candidate_id) {
                return Err(Error::InvalidContract(format!(
                    "visual vision returned unexpected candidate_id {}",
                    evaluation.candidate_id
                )));
            }
            if !found.insert(evaluation.candidate_id.clone()) {
                return Err(Error::InvalidContract(format!(
                    "visual vision returned duplicate candidate_id {}",
                    evaluation.candidate_id
                )));
            }
        }

        if found != expected {
            return Err(Error::InvalidContract(
                "visual vision did not evaluate every requested candidate".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum VisualVisionOutcome {
    Applied {
        model: String,
        route_id: String,
        result: VisualVisionEvaluationSet,
    },
    Skipped {
        reason: String,
        route_id: Option<String>,
    },
}

pub fn evaluate_ranked_visual_previews(
    client: &LlmGatewayClient,
    scene: &SceneIntentV1,
    ranked: &[RankedVisualCandidate],
    options: &VisualVisionOptions,
) -> Result<VisualVisionOutcome> {
    scene.validate_v1()?;
    options.validate()?;

    let model = options
        .model
        .clone()
        .unwrap_or_else(|| client.config().default_model.clone());
    let (body, candidate_ids) = build_visual_vision_request(scene, ranked, &model, options)?;

    if candidate_ids.is_empty() {
        return Ok(VisualVisionOutcome::Skipped {
            reason: "no ranked candidate has an image preview suitable for vision evaluation"
                .to_owned(),
            route_id: None,
        });
    }

    let trace = client.explain_routes_for_body(&model, &body)?;
    let Some(selected) = trace.selected_candidate() else {
        return Ok(VisualVisionOutcome::Skipped {
            reason: "LLMGateway route explain found no selected route".to_owned(),
            route_id: None,
        });
    };
    if selected.transport != "api" {
        return Ok(VisualVisionOutcome::Skipped {
            reason: format!(
                "selected LLMGateway transport '{}' is not vision-safe; browser adapters are text-only",
                selected.transport
            ),
            route_id: Some(selected.route_id.clone()),
        });
    }

    let routed = client.chat_raw_routed(&body)?;
    let Some(actual_route_id) = routed.route_id else {
        return Ok(VisualVisionOutcome::Skipped {
            reason: "LLMGateway response did not expose the actual route id".to_owned(),
            route_id: None,
        });
    };

    if trace.transport_for_route(&actual_route_id) != Some("api") {
        return Ok(VisualVisionOutcome::Skipped {
            reason: "actual LLMGateway route was not confirmed as API transport".to_owned(),
            route_id: Some(actual_route_id),
        });
    }

    let result =
        decode_structured_output::<VisualVisionEvaluationSet, _>(&routed.chat.content, &|result| {
            result.validate_for_candidates(&candidate_ids)
        })
        .map_err(|reason| Error::InvalidStructuredOutput {
            contract: VISUAL_VISION_CONTRACT.to_owned(),
            attempts: 1,
            reason,
        })?;

    Ok(VisualVisionOutcome::Applied {
        model,
        route_id: actual_route_id,
        result,
    })
}

pub(crate) fn build_visual_vision_request(
    scene: &SceneIntentV1,
    ranked: &[RankedVisualCandidate],
    model: &str,
    options: &VisualVisionOptions,
) -> Result<(Value, Vec<String>)> {
    scene.validate_v1()?;
    options.validate()?;
    if model.trim().is_empty() {
        return Err(Error::InvalidLlmGatewayConfig(
            "visual vision model must not be empty".to_owned(),
        ));
    }

    let mut content = vec![serde_json::json!({
        "type":"text",
        "text": visual_vision_prompt(scene)
    })];
    let mut candidate_ids = Vec::new();

    for ranked_candidate in ranked.iter().take(options.max_candidates) {
        ranked_candidate.candidate.validate()?;
        if ranked_candidate.candidate.scene_id != scene.id {
            return Err(Error::InvalidContract(format!(
                "visual vision candidate {} belongs to scene {}, expected {}",
                ranked_candidate.candidate.candidate_id,
                ranked_candidate.candidate.scene_id,
                scene.id
            )));
        }
        let Some(preview) = vision_preview(&ranked_candidate.candidate) else {
            continue;
        };

        candidate_ids.push(ranked_candidate.candidate.candidate_id.clone());
        content.push(serde_json::json!({
            "type":"text",
            "text": format!(
                "Candidate {}. Evaluate the following preview for this candidate.",
                ranked_candidate.candidate.candidate_id
            )
        }));
        content.push(serde_json::json!({
            "type":"image_url",
            "image_url":{
                "url":preview.url,
                "detail":"low"
            }
        }));
    }

    let body = serde_json::json!({
        "model": model,
        "messages":[
            {
                "role":"system",
                "content":"You are OmniCreator's provider-neutral visual reviewer. Judge only what is visible in the supplied previews and the supplied SceneIntent. Do not infer provider-specific metadata. Return JSON only."
            },
            {
                "role":"user",
                "content":content
            }
        ],
        "stream":false,
        "llmgateway_task":"reasoning",
        "temperature":0.0,
        "max_tokens":options.max_tokens
    });

    Ok((body, candidate_ids))
}

fn visual_vision_prompt(scene: &SceneIntentV1) -> String {
    format!(
        "SceneIntent:\n- scene_id: {}\n- narration: {}\n- purpose: {}\n- scene_type: {}\n- emotion_before: {}\n- emotion_after: {}\n- avoid: {}\n\nEvaluate each labeled candidate image. Return exactly one JSON object with this shape: {{\"evaluations\":[{{\"candidate_id\":\"...\",\"semantic_relevance\":0.0,\"emotional_relevance\":0.0,\"narrative_purpose\":0.0,\"visual_quality\":0.0,\"editability\":0.0,\"rationale\":\"one concise sentence\"}}]}}. Scores are 0.0 to 1.0. Include every supplied candidate exactly once. Penalize generic or cliché imagery when it conflicts with the avoid guidance.",
        scene.id,
        scene.narration,
        scene.purpose,
        scene.scene_type,
        scene.emotion_before.as_deref().unwrap_or("unspecified"),
        scene.emotion_after.as_deref().unwrap_or("unspecified"),
        scene.avoid.join(", ")
    )
}

fn vision_preview(candidate: &VisualCandidate) -> Option<VisualCandidatePreview> {
    for kind in [VisualPreviewKind::Thumbnail, VisualPreviewKind::Image] {
        if let Some(preview) = candidate
            .previews
            .iter()
            .find(|preview| preview.kind == kind)
        {
            return Some(preview.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        LlmGatewayRouteCandidate, LlmGatewayRouteTrace, VisualCandidateScore, VisualMediaType,
        SCENE_INTENT_SCHEMA, SCENE_INTENT_SCHEMA_VERSION,
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

    fn ranked(id: &str) -> RankedVisualCandidate {
        RankedVisualCandidate {
            candidate: VisualCandidate {
                candidate_id: id.to_owned(),
                scene_id: "SC17".to_owned(),
                source_provider: "fixture-provider".to_owned(),
                source_asset_id: format!("asset-{id}"),
                selection_ref: format!("fixture:image:{id}"),
                media_type: VisualMediaType::Image,
                title: Some(format!("Candidate {id}")),
                description: None,
                tags: Vec::new(),
                source_page_url: Some(format!("https://provider.example/{id}")),
                creator_name: Some("Fixture Creator".to_owned()),
                creator_url: None,
                width: Some(1280),
                height: Some(720),
                duration: None,
                previews: vec![VisualCandidatePreview {
                    kind: VisualPreviewKind::Thumbnail,
                    url: format!("https://preview.example/{id}.jpg"),
                    width: Some(640),
                    height: Some(360),
                    duration: None,
                }],
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
                cliche_matches: Vec::new(),
                cliche_penalty: 0.0,
                reuse_penalty: 0.0,
                final_score: 0.83,
            },
        }
    }

    #[test]
    fn multimodal_request_is_provider_neutral_and_preview_only() {
        let (body, candidate_ids) = build_visual_vision_request(
            &scene(),
            &[ranked("A"), ranked("B")],
            "vision-model",
            &VisualVisionOptions::default(),
        )
        .unwrap();

        assert_eq!(candidate_ids, vec!["A", "B"]);
        assert_eq!(body["model"], "vision-model");
        assert_eq!(body["stream"], false);

        let encoded = body.to_string();
        assert!(encoded.contains("image_url"));
        assert!(encoded.contains("https://preview.example/A.jpg"));
        assert!(encoded.contains("https://preview.example/B.jpg"));
        assert!(!encoded.contains("fixture-provider"));
        assert!(!encoded.contains("asset-A"));
        assert!(!encoded.contains("selection_ref"));
        assert!(!encoded.contains("provider.example"));
    }

    #[test]
    fn evaluation_contract_requires_every_candidate_exactly_once() {
        let result = VisualVisionEvaluationSet {
            evaluations: vec![evaluation("A", 0.8), evaluation("B", 0.7)],
        };
        result
            .validate_for_candidates(&["A".to_owned(), "B".to_owned()])
            .unwrap();

        let duplicate = VisualVisionEvaluationSet {
            evaluations: vec![evaluation("A", 0.8), evaluation("A", 0.7)],
        };
        assert!(duplicate
            .validate_for_candidates(&["A".to_owned(), "B".to_owned()])
            .is_err());
    }

    #[test]
    fn fit_score_is_balanced_across_visual_dimensions() {
        let evaluation = VisualVisionEvaluation {
            candidate_id: "A".to_owned(),
            semantic_relevance: 1.0,
            emotional_relevance: 0.8,
            narrative_purpose: 0.6,
            visual_quality: 0.4,
            editability: 0.2,
            rationale: "Balanced fixture.".to_owned(),
        };

        assert!((evaluation.fit_score() - 0.6).abs() < 1e-9);
    }

    #[test]
    fn route_trace_distinguishes_api_from_browser_for_vision() {
        let trace = LlmGatewayRouteTrace {
            selected_route: Some("api-route".to_owned()),
            candidates: vec![
                LlmGatewayRouteCandidate {
                    route_id: "browser-route".to_owned(),
                    transport: "browser".to_owned(),
                    eligible: true,
                    selected: false,
                },
                LlmGatewayRouteCandidate {
                    route_id: "api-route".to_owned(),
                    transport: "api".to_owned(),
                    eligible: true,
                    selected: true,
                },
            ],
        };

        assert_eq!(trace.selected_transport(), Some("api"));
        assert_eq!(trace.transport_for_route("browser-route"), Some("browser"));
        assert_eq!(trace.transport_for_route("api-route"), Some("api"));
    }

    fn evaluation(id: &str, score: f64) -> VisualVisionEvaluation {
        VisualVisionEvaluation {
            candidate_id: id.to_owned(),
            semantic_relevance: score,
            emotional_relevance: score,
            narrative_purpose: score,
            visual_quality: score,
            editability: score,
            rationale: format!("Candidate {id} fixture rationale."),
        }
    }
}
