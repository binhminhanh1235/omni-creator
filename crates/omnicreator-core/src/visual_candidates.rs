use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{Error, Result, SceneIntentV1};

pub const DEFAULT_SEMANTIC_RELEVANCE_WEIGHT: f64 = 0.35;
pub const DEFAULT_EMOTIONAL_RELEVANCE_WEIGHT: f64 = 0.20;
pub const DEFAULT_NARRATIVE_PURPOSE_WEIGHT: f64 = 0.15;
pub const DEFAULT_VISUAL_QUALITY_WEIGHT: f64 = 0.10;
pub const DEFAULT_CHANNEL_CONTINUITY_WEIGHT: f64 = 0.10;
pub const DEFAULT_EDITABILITY_WEIGHT: f64 = 0.05;
pub const DEFAULT_FRESHNESS_WEIGHT: f64 = 0.05;

pub const DEFAULT_CLICHE_PENALTY_PER_MATCH: f64 = 0.08;
pub const DEFAULT_CLICHE_PENALTY_CAP: f64 = 0.24;
pub const DEFAULT_REUSE_PENALTY_PER_USE: f64 = 0.02;
pub const DEFAULT_REUSE_PENALTY_CAP: f64 = 0.10;
pub const DEFAULT_RECENT_USE_PENALTY: f64 = 0.05;

pub const DEFAULT_CLICHE_TERMS: [&str; 6] = [
    "praying hands",
    "open bible",
    "church silhouette",
    "cross",
    "person staring at sky",
    "sun rays",
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisualMediaType {
    Image,
    Video,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisualPreviewKind {
    Thumbnail,
    Image,
    Video,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualCandidatePreview {
    pub kind: VisualPreviewKind,
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<f64>,
}

impl VisualCandidatePreview {
    pub fn validate(&self) -> Result<()> {
        require_non_empty("visual candidate preview url", &self.url)?;
        if self.width == Some(0) {
            return Err(Error::InvalidContract(
                "visual candidate preview width must be positive when present".to_owned(),
            ));
        }
        if self.height == Some(0) {
            return Err(Error::InvalidContract(
                "visual candidate preview height must be positive when present".to_owned(),
            ));
        }
        if self
            .duration
            .is_some_and(|duration| !duration.is_finite() || duration <= 0.0)
        {
            return Err(Error::InvalidContract(
                "visual candidate preview duration must be finite and positive when present"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

/// Provider-neutral search result.
///
/// This deliberately contains preview metadata and an opaque selection reference, but no local
/// full-asset path. Search/ranking therefore cannot accidentally imply that the source media was
/// downloaded. A provider resolves `selection_ref` only after core/user selection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualCandidate {
    pub candidate_id: String,
    pub scene_id: String,
    pub source_provider: String,
    pub source_asset_id: String,
    pub selection_ref: String,
    pub media_type: VisualMediaType,
    pub title: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub source_page_url: Option<String>,
    pub creator_name: Option<String>,
    pub creator_url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<f64>,
    #[serde(default)]
    pub previews: Vec<VisualCandidatePreview>,
}

impl VisualCandidate {
    pub fn validate(&self) -> Result<()> {
        require_non_empty("visual candidate id", &self.candidate_id)?;
        require_non_empty("visual candidate scene_id", &self.scene_id)?;
        require_non_empty("visual candidate source_provider", &self.source_provider)?;
        require_non_empty("visual candidate source_asset_id", &self.source_asset_id)?;
        require_non_empty("visual candidate selection_ref", &self.selection_ref)?;
        for (label, value) in [
            ("visual candidate source_page_url", &self.source_page_url),
            ("visual candidate creator_name", &self.creator_name),
            ("visual candidate creator_url", &self.creator_url),
        ] {
            if value.as_ref().is_some_and(|value| value.trim().is_empty()) {
                return Err(Error::InvalidContract(format!(
                    "{label} must not be empty when present"
                )));
            }
        }

        if self.width == Some(0) {
            return Err(Error::InvalidContract(
                "visual candidate width must be positive when present".to_owned(),
            ));
        }
        if self.height == Some(0) {
            return Err(Error::InvalidContract(
                "visual candidate height must be positive when present".to_owned(),
            ));
        }
        if self
            .duration
            .is_some_and(|duration| !duration.is_finite() || duration <= 0.0)
        {
            return Err(Error::InvalidContract(
                "visual candidate duration must be finite and positive when present".to_owned(),
            ));
        }
        if self.previews.is_empty() {
            return Err(Error::InvalidContract(
                "visual candidate must expose at least one preview".to_owned(),
            ));
        }
        for preview in &self.previews {
            preview.validate()?;
        }
        if self.tags.iter().any(|tag| tag.trim().is_empty()) {
            return Err(Error::InvalidContract(
                "visual candidate tags must not contain empty entries".to_owned(),
            ));
        }
        Ok(())
    }

    fn searchable_text(&self) -> String {
        let mut parts = Vec::new();
        if let Some(title) = &self.title {
            parts.push(title.as_str());
        }
        if let Some(description) = &self.description {
            parts.push(description.as_str());
        }
        parts.extend(self.tags.iter().map(String::as_str));
        normalize_text(&parts.join(" "))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct VisualCandidateSignals {
    pub semantic_relevance: f64,
    pub emotional_relevance: f64,
    pub narrative_purpose: f64,
    pub visual_quality: f64,
    pub channel_continuity: f64,
    pub editability: f64,
    pub usage_count: u32,
    pub used_recently: bool,
}

impl VisualCandidateSignals {
    pub fn validate(&self) -> Result<()> {
        for (label, value) in [
            ("semantic_relevance", self.semantic_relevance),
            ("emotional_relevance", self.emotional_relevance),
            ("narrative_purpose", self.narrative_purpose),
            ("visual_quality", self.visual_quality),
            ("channel_continuity", self.channel_continuity),
            ("editability", self.editability),
        ] {
            validate_unit_interval(label, value)?;
        }
        Ok(())
    }

    pub fn freshness(self) -> f64 {
        1.0 / (1.0 + f64::from(self.usage_count))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualCandidateRankingInput {
    pub candidate: VisualCandidate,
    pub signals: VisualCandidateSignals,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualRankingWeights {
    pub semantic_relevance: f64,
    pub emotional_relevance: f64,
    pub narrative_purpose: f64,
    pub visual_quality: f64,
    pub channel_continuity: f64,
    pub editability: f64,
    pub freshness: f64,
}

impl Default for VisualRankingWeights {
    fn default() -> Self {
        Self {
            semantic_relevance: DEFAULT_SEMANTIC_RELEVANCE_WEIGHT,
            emotional_relevance: DEFAULT_EMOTIONAL_RELEVANCE_WEIGHT,
            narrative_purpose: DEFAULT_NARRATIVE_PURPOSE_WEIGHT,
            visual_quality: DEFAULT_VISUAL_QUALITY_WEIGHT,
            channel_continuity: DEFAULT_CHANNEL_CONTINUITY_WEIGHT,
            editability: DEFAULT_EDITABILITY_WEIGHT,
            freshness: DEFAULT_FRESHNESS_WEIGHT,
        }
    }
}

impl VisualRankingWeights {
    pub fn validate(&self) -> Result<()> {
        for (label, value) in [
            ("semantic_relevance weight", self.semantic_relevance),
            ("emotional_relevance weight", self.emotional_relevance),
            ("narrative_purpose weight", self.narrative_purpose),
            ("visual_quality weight", self.visual_quality),
            ("channel_continuity weight", self.channel_continuity),
            ("editability weight", self.editability),
            ("freshness weight", self.freshness),
        ] {
            validate_unit_interval(label, value)?;
        }

        let total = self.total();
        if (total - 1.0).abs() > 1e-9 {
            return Err(Error::InvalidContract(format!(
                "visual ranking weights must sum to 1.0, found {total}"
            )));
        }
        Ok(())
    }

    pub fn total(&self) -> f64 {
        self.semantic_relevance
            + self.emotional_relevance
            + self.narrative_purpose
            + self.visual_quality
            + self.channel_continuity
            + self.editability
            + self.freshness
    }

    pub fn content_match_total(&self) -> f64 {
        self.semantic_relevance + self.emotional_relevance + self.narrative_purpose
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualRankingPolicy {
    pub weights: VisualRankingWeights,
    pub cliche_terms: Vec<String>,
    pub cliche_penalty_per_match: f64,
    pub cliche_penalty_cap: f64,
    pub reuse_penalty_per_use: f64,
    pub reuse_penalty_cap: f64,
    pub recent_use_penalty: f64,
}

impl Default for VisualRankingPolicy {
    fn default() -> Self {
        Self {
            weights: VisualRankingWeights::default(),
            cliche_terms: DEFAULT_CLICHE_TERMS
                .iter()
                .map(|term| (*term).to_owned())
                .collect(),
            cliche_penalty_per_match: DEFAULT_CLICHE_PENALTY_PER_MATCH,
            cliche_penalty_cap: DEFAULT_CLICHE_PENALTY_CAP,
            reuse_penalty_per_use: DEFAULT_REUSE_PENALTY_PER_USE,
            reuse_penalty_cap: DEFAULT_REUSE_PENALTY_CAP,
            recent_use_penalty: DEFAULT_RECENT_USE_PENALTY,
        }
    }
}

impl VisualRankingPolicy {
    pub fn validate(&self) -> Result<()> {
        self.weights.validate()?;
        for (label, value) in [
            ("cliche_penalty_per_match", self.cliche_penalty_per_match),
            ("cliche_penalty_cap", self.cliche_penalty_cap),
            ("reuse_penalty_per_use", self.reuse_penalty_per_use),
            ("reuse_penalty_cap", self.reuse_penalty_cap),
            ("recent_use_penalty", self.recent_use_penalty),
        ] {
            validate_unit_interval(label, value)?;
        }
        if self
            .cliche_terms
            .iter()
            .any(|term| normalize_text(term).is_empty())
        {
            return Err(Error::InvalidContract(
                "visual ranking cliche_terms must not contain empty entries".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualCandidateScore {
    pub semantic_relevance: f64,
    pub emotional_relevance: f64,
    pub narrative_purpose: f64,
    pub visual_quality: f64,
    pub channel_continuity: f64,
    pub editability: f64,
    pub freshness: f64,
    pub content_match_score: f64,
    pub base_score: f64,
    pub cliche_matches: Vec<String>,
    pub cliche_penalty: f64,
    pub reuse_penalty: f64,
    pub final_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RankedVisualCandidate {
    pub candidate: VisualCandidate,
    pub score: VisualCandidateScore,
}

pub fn rank_visual_candidates(
    scene: &SceneIntentV1,
    inputs: Vec<VisualCandidateRankingInput>,
    policy: &VisualRankingPolicy,
) -> Result<Vec<RankedVisualCandidate>> {
    scene.validate_v1()?;
    policy.validate()?;

    let mut candidate_ids = BTreeSet::new();
    let mut ranked = Vec::with_capacity(inputs.len());

    for input in inputs {
        input.candidate.validate()?;
        input.signals.validate()?;

        if input.candidate.scene_id != scene.id {
            return Err(Error::InvalidContract(format!(
                "visual candidate {} belongs to scene {}, expected {}",
                input.candidate.candidate_id, input.candidate.scene_id, scene.id
            )));
        }
        if !candidate_ids.insert(input.candidate.candidate_id.clone()) {
            return Err(Error::InvalidContract(format!(
                "duplicate visual candidate id: {}",
                input.candidate.candidate_id
            )));
        }

        let score = score_visual_candidate(&input.candidate, input.signals, policy);
        ranked.push(RankedVisualCandidate {
            candidate: input.candidate,
            score,
        });
    }

    ranked.sort_by(|left, right| {
        right
            .score
            .final_score
            .total_cmp(&left.score.final_score)
            .then_with(|| {
                left.candidate
                    .candidate_id
                    .cmp(&right.candidate.candidate_id)
            })
            .then_with(|| {
                left.candidate
                    .source_provider
                    .cmp(&right.candidate.source_provider)
            })
            .then_with(|| {
                left.candidate
                    .source_asset_id
                    .cmp(&right.candidate.source_asset_id)
            })
    });

    Ok(ranked)
}

pub fn score_visual_candidate(
    candidate: &VisualCandidate,
    signals: VisualCandidateSignals,
    policy: &VisualRankingPolicy,
) -> VisualCandidateScore {
    let weights = &policy.weights;
    let freshness = signals.freshness();

    let semantic_relevance = signals.semantic_relevance * weights.semantic_relevance;
    let emotional_relevance = signals.emotional_relevance * weights.emotional_relevance;
    let narrative_purpose = signals.narrative_purpose * weights.narrative_purpose;
    let visual_quality = signals.visual_quality * weights.visual_quality;
    let channel_continuity = signals.channel_continuity * weights.channel_continuity;
    let editability = signals.editability * weights.editability;
    let freshness_score = freshness * weights.freshness;

    let content_match_score = semantic_relevance + emotional_relevance + narrative_purpose;
    let base_score =
        content_match_score + visual_quality + channel_continuity + editability + freshness_score;

    let cliche_matches = matched_cliche_terms(candidate, &policy.cliche_terms);
    let cliche_penalty = (policy.cliche_penalty_per_match * cliche_matches.len() as f64)
        .min(policy.cliche_penalty_cap);
    let reuse_penalty = (policy.reuse_penalty_per_use * f64::from(signals.usage_count))
        .min(policy.reuse_penalty_cap)
        + if signals.used_recently {
            policy.recent_use_penalty
        } else {
            0.0
        };

    VisualCandidateScore {
        semantic_relevance,
        emotional_relevance,
        narrative_purpose,
        visual_quality,
        channel_continuity,
        editability,
        freshness: freshness_score,
        content_match_score,
        base_score,
        cliche_matches,
        cliche_penalty,
        reuse_penalty,
        final_score: (base_score - cliche_penalty - reuse_penalty).clamp(0.0, 1.0),
    }
}

fn matched_cliche_terms(candidate: &VisualCandidate, terms: &[String]) -> Vec<String> {
    let searchable = candidate.searchable_text();
    let searchable_tokens = token_set(&searchable);
    let mut matches = terms
        .iter()
        .filter_map(|term| {
            let normalized = normalize_text(term);
            if normalized.is_empty() {
                return None;
            }
            let tokens = token_set(&normalized);
            if !tokens.is_empty() && tokens.is_subset(&searchable_tokens) {
                Some(term.trim().to_owned())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    matches
}

fn token_set(value: &str) -> BTreeSet<&str> {
    value.split_whitespace().collect()
}

fn normalize_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn validate_unit_interval(label: &str, value: f64) -> Result<()> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(Error::InvalidContract(format!(
            "{label} must be finite and between 0.0 and 1.0"
        )));
    }
    Ok(())
}

fn require_non_empty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidContract(format!("{label} must not be empty")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{SCENE_INTENT_SCHEMA, SCENE_INTENT_SCHEMA_VERSION};

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

    fn candidate(id: &str, title: &str) -> VisualCandidate {
        VisualCandidate {
            candidate_id: id.to_owned(),
            scene_id: "SC17".to_owned(),
            source_provider: "fixture".to_owned(),
            source_asset_id: format!("asset-{id}"),
            selection_ref: format!("fixture:{id}"),
            media_type: VisualMediaType::Video,
            title: Some(title.to_owned()),
            description: None,
            tags: Vec::new(),
            source_page_url: Some(format!("https://example.invalid/{id}")),
            creator_name: Some("Fixture Creator".to_owned()),
            creator_url: Some("https://example.invalid/creator".to_owned()),
            width: Some(1920),
            height: Some(1080),
            duration: Some(12.0),
            previews: vec![VisualCandidatePreview {
                kind: VisualPreviewKind::Thumbnail,
                url: format!("https://example.invalid/{id}.jpg"),
                width: Some(640),
                height: Some(360),
                duration: None,
            }],
        }
    }

    fn signals() -> VisualCandidateSignals {
        VisualCandidateSignals {
            semantic_relevance: 0.9,
            emotional_relevance: 0.8,
            narrative_purpose: 0.9,
            visual_quality: 0.8,
            channel_continuity: 0.7,
            editability: 0.8,
            usage_count: 0,
            used_recently: false,
        }
    }

    #[test]
    fn default_weights_match_scene_intelligence_contract() {
        let weights = VisualRankingWeights::default();
        weights.validate().unwrap();

        assert!((weights.total() - 1.0).abs() < 1e-9);
        assert!((weights.content_match_total() - 0.70).abs() < 1e-9);
        assert_eq!(weights.semantic_relevance, 0.35);
        assert_eq!(weights.emotional_relevance, 0.20);
        assert_eq!(weights.narrative_purpose, 0.15);
    }

    #[test]
    fn candidate_requires_preview_but_not_full_asset_path() {
        let mut value = candidate("A", "repairing a wooden bridge");
        value.validate().unwrap();

        value.previews.clear();
        let error = value.validate().unwrap_err();
        assert!(error.to_string().contains("at least one preview"));
    }

    #[test]
    fn scoring_exposes_content_match_and_breakdown() {
        let candidate = candidate("A", "repairing a wooden bridge");
        let score = score_visual_candidate(&candidate, signals(), &VisualRankingPolicy::default());

        assert!((score.content_match_score - 0.61).abs() < 1e-9);
        assert!(score.base_score > score.content_match_score);
        assert!(score.cliche_matches.is_empty());
        assert_eq!(score.cliche_penalty, 0.0);
        assert_eq!(score.reuse_penalty, 0.0);
        assert_eq!(score.final_score, score.base_score);
    }

    #[test]
    fn cliche_penalty_lowers_otherwise_equal_candidate() {
        let normal = VisualCandidateRankingInput {
            candidate: candidate("A", "craftsperson carefully rebuilding a fence"),
            signals: signals(),
        };
        let cliche = VisualCandidateRankingInput {
            candidate: candidate("B", "praying hands beside an open Bible and cross"),
            signals: signals(),
        };

        let ranked = rank_visual_candidates(
            &scene(),
            vec![cliche, normal],
            &VisualRankingPolicy::default(),
        )
        .unwrap();

        assert_eq!(ranked[0].candidate.candidate_id, "A");
        assert_eq!(ranked[1].score.cliche_matches.len(), 3);
        assert!(ranked[1].score.cliche_penalty > 0.0);
    }

    #[test]
    fn reuse_and_recent_use_lower_candidate_score() {
        let fresh = VisualCandidateRankingInput {
            candidate: candidate("fresh", "repairing fence"),
            signals: signals(),
        };
        let mut reused_signals = signals();
        reused_signals.usage_count = 4;
        reused_signals.used_recently = true;
        let reused = VisualCandidateRankingInput {
            candidate: candidate("reused", "repairing fence"),
            signals: reused_signals,
        };

        let ranked = rank_visual_candidates(
            &scene(),
            vec![reused, fresh],
            &VisualRankingPolicy::default(),
        )
        .unwrap();

        assert_eq!(ranked[0].candidate.candidate_id, "fresh");
        assert!(ranked[1].score.reuse_penalty > 0.0);
        assert!(ranked[1].score.freshness < ranked[0].score.freshness);
    }

    #[test]
    fn equal_scores_use_stable_candidate_id_tie_break() {
        let ranked = rank_visual_candidates(
            &scene(),
            vec![
                VisualCandidateRankingInput {
                    candidate: candidate("B", "repairing fence"),
                    signals: signals(),
                },
                VisualCandidateRankingInput {
                    candidate: candidate("A", "repairing fence"),
                    signals: signals(),
                },
            ],
            &VisualRankingPolicy::default(),
        )
        .unwrap();

        assert_eq!(
            ranked
                .iter()
                .map(|candidate| candidate.candidate.candidate_id.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "B"]
        );
    }

    #[test]
    fn candidate_for_other_scene_is_rejected() {
        let mut wrong = candidate("A", "repairing fence");
        wrong.scene_id = "SC99".to_owned();

        let error = rank_visual_candidates(
            &scene(),
            vec![VisualCandidateRankingInput {
                candidate: wrong,
                signals: signals(),
            }],
            &VisualRankingPolicy::default(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("expected SC17"));
    }

    #[test]
    fn invalid_signal_is_rejected_before_ranking() {
        let mut invalid = signals();
        invalid.semantic_relevance = 1.1;

        let error = rank_visual_candidates(
            &scene(),
            vec![VisualCandidateRankingInput {
                candidate: candidate("A", "repairing fence"),
                signals: invalid,
            }],
            &VisualRankingPolicy::default(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("semantic_relevance"));
    }

    #[test]
    fn duplicate_candidate_ids_are_rejected() {
        let error = rank_visual_candidates(
            &scene(),
            vec![
                VisualCandidateRankingInput {
                    candidate: candidate("A", "first"),
                    signals: signals(),
                },
                VisualCandidateRankingInput {
                    candidate: candidate("A", "second"),
                    signals: signals(),
                },
            ],
            &VisualRankingPolicy::default(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("duplicate visual candidate id"));
    }
}
