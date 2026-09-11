use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRecoveryActionKindV1 {
    Retry,
    Configure,
    ChooseAlternative,
    ProvideManually,
    ReplaceResult,
    Details,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRecoveryBlockerClassV1 {
    RetryableExecution,
    ConfigurationRequired,
    ProviderUnavailable,
    ManualInputRequired,
    SelectedResultInvalid,
    ValidationError,
    DependencyBlocked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRecoveryStageV1 {
    Content,
    ScenePlan,
    Visual,
    Voice,
    ProductionPack,
    Export,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRecoveryResultStateV1 {
    None,
    Verified,
    Missing,
    Invalid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRecoveryActionTargetV1 {
    RetryCanonicalJob,
    ConfigurationSurface,
    AlternativeProvider,
    ManualTakeover,
    ExternalHandoff,
    ReplaceSelectedResult,
    ProductionRecovery,
    Details,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRecoveryAlternativeStateV1 {
    Ready,
    SetupRequired,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewRecoveryAlternativeV1 {
    pub id: String,
    pub capability: String,
    pub state: ReviewRecoveryAlternativeStateV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewRecoveryActionV1 {
    pub kind: ReviewRecoveryActionKindV1,
    pub target: ReviewRecoveryActionTargetV1,
    pub label: String,
    pub primary: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewRecoveryContextV1 {
    pub stage: ReviewRecoveryStageV1,
    pub blocker_class: ReviewRecoveryBlockerClassV1,
    pub result_state: ReviewRecoveryResultStateV1,
    #[serde(default)]
    pub required_capability: Option<String>,
    #[serde(default)]
    pub alternatives: Vec<ReviewRecoveryAlternativeV1>,
    /// True only when product semantics require an explicit user selection.
    /// Ordered Studio Pack fallback must leave this false.
    pub explicit_alternative_choice: bool,
    pub retry_available: bool,
    pub configure_available: bool,
    pub manual_available: bool,
    pub external_handoff_available: bool,
    pub production_recovery_available: bool,
    pub read_only: bool,
}

impl ReviewRecoveryContextV1 {
    pub fn has_invalid_selected_result_v1(&self) -> bool {
        matches!(
            self.result_state,
            ReviewRecoveryResultStateV1::Missing | ReviewRecoveryResultStateV1::Invalid
        )
    }
}

pub fn derive_review_recovery_actions_v1(
    context: &ReviewRecoveryContextV1,
) -> Vec<ReviewRecoveryActionV1> {
    let mut actions = Vec::new();
    let mutation_enabled = !context.read_only;
    let replace_applicable = context.has_invalid_selected_result_v1();

    if replace_applicable {
        push_action_v1(
            &mut actions,
            ReviewRecoveryActionKindV1::ReplaceResult,
            if context.production_recovery_available {
                ReviewRecoveryActionTargetV1::ProductionRecovery
            } else {
                ReviewRecoveryActionTargetV1::ReplaceSelectedResult
            },
            replace_label_v1(context.stage),
            true,
            mutation_enabled,
        );
    }

    let manual_is_preferred = matches!(
        context.blocker_class,
        ReviewRecoveryBlockerClassV1::ProviderUnavailable
            | ReviewRecoveryBlockerClassV1::ManualInputRequired
            | ReviewRecoveryBlockerClassV1::ConfigurationRequired
    ) && matches!(
        context.stage,
        ReviewRecoveryStageV1::Content
            | ReviewRecoveryStageV1::ScenePlan
            | ReviewRecoveryStageV1::Visual
            | ReviewRecoveryStageV1::Voice
    );

    if context.manual_available && !replace_applicable {
        push_action_v1(
            &mut actions,
            ReviewRecoveryActionKindV1::ProvideManually,
            ReviewRecoveryActionTargetV1::ManualTakeover,
            manual_label_v1(context.stage),
            manual_is_preferred,
            mutation_enabled,
        );
    }

    if context.external_handoff_available && !replace_applicable {
        push_action_v1(
            &mut actions,
            ReviewRecoveryActionKindV1::ProvideManually,
            ReviewRecoveryActionTargetV1::ExternalHandoff,
            external_label_v1(context.stage),
            false,
            mutation_enabled,
        );
    }

    if context.configure_available
        && matches!(
            context.blocker_class,
            ReviewRecoveryBlockerClassV1::ConfigurationRequired
                | ReviewRecoveryBlockerClassV1::ProviderUnavailable
        )
    {
        push_action_v1(
            &mut actions,
            ReviewRecoveryActionKindV1::Configure,
            ReviewRecoveryActionTargetV1::ConfigurationSurface,
            "Configure".to_owned(),
            !manual_is_preferred && !replace_applicable,
            mutation_enabled,
        );
    }

    if context.explicit_alternative_choice && eligible_alternative_count_v1(context) >= 2 {
        push_action_v1(
            &mut actions,
            ReviewRecoveryActionKindV1::ChooseAlternative,
            ReviewRecoveryActionTargetV1::AlternativeProvider,
            "Choose Alternative".to_owned(),
            false,
            mutation_enabled,
        );
    }

    if context.retry_available
        && !matches!(
            context.blocker_class,
            ReviewRecoveryBlockerClassV1::ManualInputRequired
                | ReviewRecoveryBlockerClassV1::ValidationError
        )
    {
        let retry_is_primary = actions.iter().all(|action| !action.primary);
        push_action_v1(
            &mut actions,
            ReviewRecoveryActionKindV1::Retry,
            ReviewRecoveryActionTargetV1::RetryCanonicalJob,
            "Retry".to_owned(),
            retry_is_primary,
            mutation_enabled,
        );
    }

    push_action_v1(
        &mut actions,
        ReviewRecoveryActionKindV1::Details,
        ReviewRecoveryActionTargetV1::Details,
        "Details".to_owned(),
        false,
        true,
    );

    if actions.iter().all(|action| !action.primary) {
        if let Some(action) = actions
            .iter_mut()
            .find(|action| action.kind != ReviewRecoveryActionKindV1::Details)
        {
            action.primary = true;
        }
    }

    let mut found_primary = false;
    for action in &mut actions {
        if action.primary {
            if found_primary {
                action.primary = false;
            } else {
                found_primary = true;
            }
        }
    }

    actions
}

fn eligible_alternative_count_v1(context: &ReviewRecoveryContextV1) -> usize {
    context
        .alternatives
        .iter()
        .filter(|candidate| {
            candidate.state == ReviewRecoveryAlternativeStateV1::Ready
                && context
                    .required_capability
                    .as_deref()
                    .map_or(true, |required| candidate.capability == required)
        })
        .count()
}

fn push_action_v1(
    actions: &mut Vec<ReviewRecoveryActionV1>,
    kind: ReviewRecoveryActionKindV1,
    target: ReviewRecoveryActionTargetV1,
    label: String,
    primary: bool,
    enabled: bool,
) {
    if actions
        .iter()
        .any(|action| action.kind == kind && action.target == target)
    {
        return;
    }
    actions.push(ReviewRecoveryActionV1 {
        kind,
        target,
        label,
        primary,
        enabled,
    });
}

fn manual_label_v1(stage: ReviewRecoveryStageV1) -> String {
    match stage {
        ReviewRecoveryStageV1::Content => "Provide Script Manually",
        ReviewRecoveryStageV1::ScenePlan => "Provide Scene Plan Manually",
        ReviewRecoveryStageV1::Visual => "Provide My Visual",
        ReviewRecoveryStageV1::Voice => "Provide My Audio",
        ReviewRecoveryStageV1::ProductionPack | ReviewRecoveryStageV1::Export => "Repair Manually",
        ReviewRecoveryStageV1::Other => "Provide Manually",
    }
    .to_owned()
}

fn external_label_v1(stage: ReviewRecoveryStageV1) -> String {
    match stage {
        ReviewRecoveryStageV1::Visual => "Provide External Image",
        ReviewRecoveryStageV1::Voice => "Provide External Audio",
        _ => "Provide External Result",
    }
    .to_owned()
}

fn replace_label_v1(stage: ReviewRecoveryStageV1) -> String {
    match stage {
        ReviewRecoveryStageV1::Visual => "Replace Visual",
        ReviewRecoveryStageV1::Voice => "Replace Audio / Timing",
        ReviewRecoveryStageV1::ProductionPack | ReviewRecoveryStageV1::Export => {
            "Repair / Replace Result"
        }
        _ => "Replace Result",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(stage: ReviewRecoveryStageV1) -> ReviewRecoveryContextV1 {
        ReviewRecoveryContextV1 {
            stage,
            blocker_class: ReviewRecoveryBlockerClassV1::ProviderUnavailable,
            result_state: ReviewRecoveryResultStateV1::None,
            required_capability: None,
            alternatives: Vec::new(),
            explicit_alternative_choice: false,
            retry_available: true,
            configure_available: true,
            manual_available: true,
            external_handoff_available: false,
            production_recovery_available: false,
            read_only: false,
        }
    }

    fn kinds(actions: &[ReviewRecoveryActionV1]) -> Vec<ReviewRecoveryActionKindV1> {
        actions.iter().map(|action| action.kind).collect()
    }

    #[test]
    fn llm_unavailable_can_configure_retry_or_continue_manually() {
        let mut input = context(ReviewRecoveryStageV1::Content);
        input.blocker_class = ReviewRecoveryBlockerClassV1::ConfigurationRequired;
        let actions = derive_review_recovery_actions_v1(&input);
        let kinds = kinds(&actions);
        assert!(kinds.contains(&ReviewRecoveryActionKindV1::Configure));
        assert!(kinds.contains(&ReviewRecoveryActionKindV1::Retry));
        assert!(kinds.contains(&ReviewRecoveryActionKindV1::ProvideManually));
        assert!(kinds.contains(&ReviewRecoveryActionKindV1::Details));
        assert_eq!(actions.iter().filter(|action| action.primary).count(), 1);
        assert_eq!(
            actions.iter().find(|action| action.primary).unwrap().label,
            "Provide Script Manually"
        );
    }

    #[test]
    fn stock_provider_unavailable_prefers_manual_visual_without_fake_alternative() {
        let input = context(ReviewRecoveryStageV1::Visual);
        let actions = derive_review_recovery_actions_v1(&input);
        assert!(!kinds(&actions).contains(&ReviewRecoveryActionKindV1::ChooseAlternative));
        assert_eq!(
            actions.iter().find(|action| action.primary).unwrap().label,
            "Provide My Visual"
        );
    }

    #[test]
    fn explicit_alternative_requires_two_ready_capability_compatible_candidates() {
        let mut input = context(ReviewRecoveryStageV1::Visual);
        input.required_capability = Some("stick_figure_visual".to_owned());
        input.explicit_alternative_choice = true;
        input.alternatives = vec![
            ReviewRecoveryAlternativeV1 {
                id: "stick-a".to_owned(),
                capability: "stick_figure_visual".to_owned(),
                state: ReviewRecoveryAlternativeStateV1::Ready,
            },
            ReviewRecoveryAlternativeV1 {
                id: "generic-image".to_owned(),
                capability: "generated_still".to_owned(),
                state: ReviewRecoveryAlternativeStateV1::Ready,
            },
        ];
        let actions = derive_review_recovery_actions_v1(&input);
        assert!(!kinds(&actions).contains(&ReviewRecoveryActionKindV1::ChooseAlternative));

        input.alternatives.push(ReviewRecoveryAlternativeV1 {
            id: "stick-b".to_owned(),
            capability: "stick_figure_visual".to_owned(),
            state: ReviewRecoveryAlternativeStateV1::Ready,
        });
        let actions = derive_review_recovery_actions_v1(&input);
        assert!(kinds(&actions).contains(&ReviewRecoveryActionKindV1::ChooseAlternative));
    }

    #[test]
    fn generated_visual_external_handoff_is_provider_neutral_manual_action() {
        let mut input = context(ReviewRecoveryStageV1::Visual);
        input.external_handoff_available = true;
        let actions = derive_review_recovery_actions_v1(&input);
        assert!(actions.iter().any(|action| {
            action.kind == ReviewRecoveryActionKindV1::ProvideManually
                && action.target == ReviewRecoveryActionTargetV1::ExternalHandoff
        }));
    }

    #[test]
    fn corrupt_selected_visual_prefers_replace_and_keeps_retry_when_available() {
        let mut input = context(ReviewRecoveryStageV1::Visual);
        input.result_state = ReviewRecoveryResultStateV1::Invalid;
        input.production_recovery_available = true;
        let actions = derive_review_recovery_actions_v1(&input);
        assert_eq!(
            actions.iter().find(|action| action.primary).unwrap().kind,
            ReviewRecoveryActionKindV1::ReplaceResult
        );
        assert!(kinds(&actions).contains(&ReviewRecoveryActionKindV1::Retry));
    }

    #[test]
    fn corrupt_audio_or_timing_projects_replace_result() {
        let mut input = context(ReviewRecoveryStageV1::Voice);
        input.result_state = ReviewRecoveryResultStateV1::Missing;
        input.production_recovery_available = true;
        let actions = derive_review_recovery_actions_v1(&input);
        assert!(actions.iter().any(|action| {
            action.kind == ReviewRecoveryActionKindV1::ReplaceResult
                && action.target == ReviewRecoveryActionTargetV1::ProductionRecovery
        }));
    }

    #[test]
    fn validation_error_does_not_offer_meaningless_retry() {
        let mut input = context(ReviewRecoveryStageV1::ScenePlan);
        input.blocker_class = ReviewRecoveryBlockerClassV1::ValidationError;
        let actions = derive_review_recovery_actions_v1(&input);
        assert!(!kinds(&actions).contains(&ReviewRecoveryActionKindV1::Retry));
    }

    #[test]
    fn manual_required_does_not_offer_retry() {
        let mut input = context(ReviewRecoveryStageV1::Voice);
        input.blocker_class = ReviewRecoveryBlockerClassV1::ManualInputRequired;
        let actions = derive_review_recovery_actions_v1(&input);
        assert!(!kinds(&actions).contains(&ReviewRecoveryActionKindV1::Retry));
        assert!(kinds(&actions).contains(&ReviewRecoveryActionKindV1::ProvideManually));
    }

    #[test]
    fn read_only_keeps_details_and_suppresses_mutation_execution() {
        let mut input = context(ReviewRecoveryStageV1::Voice);
        input.read_only = true;
        let actions = derive_review_recovery_actions_v1(&input);
        assert!(actions
            .iter()
            .filter(|action| action.kind != ReviewRecoveryActionKindV1::Details)
            .all(|action| !action.enabled));
        assert!(actions.iter().any(|action| {
            action.kind == ReviewRecoveryActionKindV1::Details && action.enabled
        }));
    }

    #[test]
    fn deterministic_fallback_does_not_pretend_user_choice_exists() {
        let mut input = context(ReviewRecoveryStageV1::Visual);
        input.required_capability = Some("stock_video".to_owned());
        input.alternatives = vec![
            ReviewRecoveryAlternativeV1 {
                id: "stock-a".to_owned(),
                capability: "stock_video".to_owned(),
                state: ReviewRecoveryAlternativeStateV1::Ready,
            },
            ReviewRecoveryAlternativeV1 {
                id: "stock-b".to_owned(),
                capability: "stock_video".to_owned(),
                state: ReviewRecoveryAlternativeStateV1::Ready,
            },
        ];
        input.explicit_alternative_choice = false;
        let actions = derive_review_recovery_actions_v1(&input);
        assert!(!kinds(&actions).contains(&ReviewRecoveryActionKindV1::ChooseAlternative));
    }

    #[test]
    fn serialized_action_payload_has_no_secret_or_machine_path_fields() {
        let actions = derive_review_recovery_actions_v1(&context(ReviewRecoveryStageV1::Visual));
        let json = serde_json::to_string(&actions)
            .unwrap()
            .to_ascii_lowercase();
        for forbidden in [
            "api_key",
            "token",
            "cookie",
            "authorization",
            "absolute_path",
            "/users/",
        ] {
            assert!(
                !json.contains(forbidden),
                "found forbidden field/value: {forbidden}"
            );
        }
    }
}
