use std::collections::BTreeSet;

use serde_json::json;

use crate::{
    Error, LlmGatewayClient, LlmGatewayTask, LlmMessage, Result, SceneIntentV1, SegmentV1,
    StructuredOutputOptions, SCENE_INTENT_SCHEMA, SCENE_INTENT_SCHEMA_VERSION,
};

pub const DEFAULT_SCENE_ASPECT_RATIO: &str = "16:9";
pub const MIN_SCENE_SEARCH_QUERIES: usize = 3;
pub const MAX_SCENE_SEARCH_QUERIES: usize = 6;
pub const MIN_SCENE_VISUAL_IDEAS: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneIntentGenerationOptions {
    pub aspect_ratio: String,
    pub model: Option<String>,
    pub max_attempts: u8,
    pub max_tokens: Option<u32>,
    pub visual_rules: Vec<String>,
    pub avoid: Vec<String>,
}

impl Default for SceneIntentGenerationOptions {
    fn default() -> Self {
        Self {
            aspect_ratio: DEFAULT_SCENE_ASPECT_RATIO.to_owned(),
            model: None,
            max_attempts: 3,
            max_tokens: Some(1_200),
            visual_rules: Vec::new(),
            avoid: Vec::new(),
        }
    }
}

impl SceneIntentGenerationOptions {
    pub fn validate(&self) -> Result<()> {
        if self.aspect_ratio.trim().is_empty() {
            return Err(Error::InvalidContract(
                "scene generation aspect_ratio must not be empty".to_owned(),
            ));
        }
        if self
            .model
            .as_ref()
            .is_some_and(|model| model.trim().is_empty())
        {
            return Err(Error::InvalidContract(
                "scene generation model must not be empty when present".to_owned(),
            ));
        }
        if !(1..=4).contains(&self.max_attempts) {
            return Err(Error::InvalidContract(
                "scene generation max_attempts must be between 1 and 4".to_owned(),
            ));
        }
        if self.max_tokens.is_some_and(|max_tokens| max_tokens == 0) {
            return Err(Error::InvalidContract(
                "scene generation max_tokens must be positive when present".to_owned(),
            ));
        }
        if self.visual_rules.iter().any(|rule| rule.trim().is_empty()) {
            return Err(Error::InvalidContract(
                "scene generation visual_rules must not contain empty entries".to_owned(),
            ));
        }
        if self.avoid.iter().any(|rule| rule.trim().is_empty()) {
            return Err(Error::InvalidContract(
                "scene generation avoid must not contain empty entries".to_owned(),
            ));
        }
        Ok(())
    }
}

impl LlmGatewayClient {
    pub fn generate_scene_intent(
        &self,
        segment: &SegmentV1,
        scene_id: &str,
        options: &SceneIntentGenerationOptions,
    ) -> Result<SceneIntentV1> {
        segment.validate_v1()?;
        options.validate()?;
        if scene_id.trim().is_empty() {
            return Err(Error::InvalidContract(
                "scene generation scene_id must not be empty".to_owned(),
            ));
        }

        let messages = build_scene_intent_messages(segment, scene_id, options)?;
        let structured = structured_scene_options(options);

        self.chat_structured(messages, &structured, |scene: &SceneIntentV1| {
            validate_generated_scene_intent(scene, segment, scene_id, options)
        })
    }
}

pub fn validate_generated_scene_intent(
    scene: &SceneIntentV1,
    segment: &SegmentV1,
    scene_id: &str,
    options: &SceneIntentGenerationOptions,
) -> Result<()> {
    segment.validate_v1()?;
    options.validate()?;
    scene.validate_v1()?;

    if scene.id != scene_id {
        return Err(Error::InvalidContract(format!(
            "generated scene id must be {scene_id}, found {}",
            scene.id
        )));
    }
    if scene.segment_id != segment.id {
        return Err(Error::InvalidContract(format!(
            "generated scene segment_id must be {}, found {}",
            segment.id, scene.segment_id
        )));
    }
    if scene.narration != segment.text {
        return Err(Error::InvalidContract(
            "generated scene narration must exactly preserve segment text".to_owned(),
        ));
    }
    if !matches!(
        scene.scene_type.as_str(),
        "literal" | "emotional" | "conceptual"
    ) {
        return Err(Error::InvalidContract(
            "generated scene_type must be literal, emotional, or conceptual".to_owned(),
        ));
    }
    if scene.aspect_ratio != options.aspect_ratio {
        return Err(Error::InvalidContract(format!(
            "generated scene aspect_ratio must be {}, found {}",
            options.aspect_ratio, scene.aspect_ratio
        )));
    }
    if scene.visual_ideas.len() < MIN_SCENE_VISUAL_IDEAS {
        return Err(Error::InvalidContract(format!(
            "generated scene must contain at least {MIN_SCENE_VISUAL_IDEAS} visual ideas"
        )));
    }
    if !(MIN_SCENE_SEARCH_QUERIES..=MAX_SCENE_SEARCH_QUERIES).contains(&scene.search_queries.len())
    {
        return Err(Error::InvalidContract(format!(
            "generated scene must contain between {MIN_SCENE_SEARCH_QUERIES} and {MAX_SCENE_SEARCH_QUERIES} search queries"
        )));
    }
    if scene
        .visual_ideas
        .iter()
        .chain(scene.search_queries.iter())
        .chain(scene.avoid.iter())
        .any(|value| value.trim().is_empty())
    {
        return Err(Error::InvalidContract(
            "generated scene lists must not contain empty entries".to_owned(),
        ));
    }

    let mut unique_queries = BTreeSet::new();
    for query in &scene.search_queries {
        let normalized = query.trim().to_lowercase();
        if !unique_queries.insert(normalized) {
            return Err(Error::InvalidContract(
                "generated scene search queries must be distinct".to_owned(),
            ));
        }
    }

    for required in &options.avoid {
        if !scene
            .avoid
            .iter()
            .any(|actual| actual.eq_ignore_ascii_case(required))
        {
            return Err(Error::InvalidContract(format!(
                "generated scene avoid list must preserve required rule: {required}"
            )));
        }
    }

    Ok(())
}

fn build_scene_intent_messages(
    segment: &SegmentV1,
    scene_id: &str,
    options: &SceneIntentGenerationOptions,
) -> Result<Vec<LlmMessage>> {
    let input = json!({
        "scene_id": scene_id,
        "segment": segment,
        "aspect_ratio": options.aspect_ratio,
        "visual_rules": options.visual_rules,
        "required_avoid": options.avoid,
    });
    let input_json = serde_json::to_string_pretty(&input)?;
    let user_prompt = format!(
        "Create exactly one SceneIntent v1 object for the supplied segment.\n\
The output must use schema '{SCENE_INTENT_SCHEMA}' with schema_version {SCENE_INTENT_SCHEMA_VERSION}.\n\
Preserve scene_id, segment_id, narration, and aspect_ratio exactly from the input.\n\
Choose scene_type from literal, emotional, or conceptual.\n\
Provide at least {MIN_SCENE_VISUAL_IDEAS} distinct visual_ideas.\n\
Provide {MIN_SCENE_SEARCH_QUERIES} to {MAX_SCENE_SEARCH_QUERIES} distinct search_queries that describe concrete shootable or searchable visuals, not spiritual abstractions or keyword soup.\n\
Carry every required_avoid rule into the output avoid list.\n\
Prefer metaphor and emotional specificity when literal religious stock imagery would be repetitive.\n\
Return JSON only.\n\
Input:\n{input_json}"
    );

    Ok(vec![
        LlmMessage::system(
            "You are OmniCreator Scene Intelligence. Translate narration meaning into a provider-neutral visual intent. Do not choose providers, accounts, URLs, or physical model routes. Avoid repetitive religious stock cliches unless the narration specifically requires them.",
        ),
        LlmMessage::user(user_prompt),
    ])
}

fn structured_scene_options(options: &SceneIntentGenerationOptions) -> StructuredOutputOptions {
    let mut structured = StructuredOutputOptions::new(format!(
        "{SCENE_INTENT_SCHEMA}.v{SCENE_INTENT_SCHEMA_VERSION}"
    ));
    structured.model = options.model.clone();
    structured.task = LlmGatewayTask::Reasoning;
    structured.temperature = Some(0.2);
    structured.max_tokens = options.max_tokens;
    structured.max_attempts = options.max_attempts;
    structured
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{VoiceDirectionV1, SEGMENT_SCHEMA, SEGMENT_SCHEMA_VERSION};

    use super::*;

    fn segment() -> SegmentV1 {
        SegmentV1 {
            schema: SEGMENT_SCHEMA.to_owned(),
            schema_version: SEGMENT_SCHEMA_VERSION,
            id: "S04".to_owned(),
            order: 4,
            text: "Forgiveness does not automatically restore trust.".to_owned(),
            voice_direction: VoiceDirectionV1::default(),
        }
    }

    fn valid_scene() -> SceneIntentV1 {
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
            visual_ideas: vec![
                "repairing a broken bridge".to_owned(),
                "rebuilding a fence".to_owned(),
            ],
            search_queries: vec![
                "repairing wooden bridge morning".to_owned(),
                "rebuilding fence careful hands".to_owned(),
                "craftsperson restoring old gate".to_owned(),
            ],
            avoid: vec!["generic praying hands".to_owned()],
            continuity: BTreeMap::new(),
            aspect_ratio: "16:9".to_owned(),
        }
    }

    #[test]
    fn scene_generation_defaults_are_provider_neutral_and_bounded() {
        let options = SceneIntentGenerationOptions::default();
        options.validate().unwrap();
        let structured = structured_scene_options(&options);

        assert!(options.model.is_none());
        assert_eq!(structured.task, LlmGatewayTask::Reasoning);
        assert_eq!(structured.max_attempts, 3);
        assert_eq!(structured.temperature, Some(0.2));
    }

    #[test]
    fn scene_prompt_requires_multiple_concrete_queries_and_no_provider_routing() {
        let messages = build_scene_intent_messages(
            &segment(),
            "SC17",
            &SceneIntentGenerationOptions::default(),
        )
        .unwrap();
        let text = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("3 to 6 distinct search_queries"));
        assert!(text.contains("concrete shootable or searchable visuals"));
        assert!(text.contains("Do not choose providers"));
        assert!(text.contains("Forgiveness does not automatically restore trust."));
    }

    #[test]
    fn generated_scene_validation_accepts_contract_aligned_output() {
        let options = SceneIntentGenerationOptions {
            avoid: vec!["generic praying hands".to_owned()],
            ..Default::default()
        };

        validate_generated_scene_intent(&valid_scene(), &segment(), "SC17", &options).unwrap();
    }

    #[test]
    fn generated_scene_validation_rejects_rewritten_narration() {
        let mut scene = valid_scene();
        scene.narration = "Trust returns immediately.".to_owned();

        let error = validate_generated_scene_intent(
            &scene,
            &segment(),
            "SC17",
            &SceneIntentGenerationOptions::default(),
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("must exactly preserve segment text"));
    }

    #[test]
    fn generated_scene_validation_rejects_duplicate_search_queries() {
        let mut scene = valid_scene();
        scene.search_queries[2] = "Rebuilding Fence Careful Hands".to_owned();

        let error = validate_generated_scene_intent(
            &scene,
            &segment(),
            "SC17",
            &SceneIntentGenerationOptions::default(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("must be distinct"));
    }

    #[test]
    fn generated_scene_validation_preserves_required_avoid_rules() {
        let options = SceneIntentGenerationOptions {
            avoid: vec!["church silhouette".to_owned()],
            ..Default::default()
        };

        let error = validate_generated_scene_intent(&valid_scene(), &segment(), "SC17", &options)
            .unwrap_err();

        assert!(error.to_string().contains("church silhouette"));
    }
}
