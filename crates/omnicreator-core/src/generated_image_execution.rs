use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    DiscoveredPlugin, Error, GeneratedImagePreflightIssueCodeV1, GeneratedImagePreparationV1,
    GpuQueueEligibilityStatusV1, GpuQueueEligibilityV1, ResourceRequirement, Result,
};

pub const GENERATED_IMAGE_EXECUTION_DECISION_SCHEMA_V1: &str =
    "omnicreator.generated-image-execution-decision";
pub const GENERATED_IMAGE_API_EXECUTION_CAPABILITY_V1: &str = "api_execution";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedImageExecutionTargetV1 {
    LocalPlugin,
    Api,
    ComputeProvider,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedImageCredentialAvailabilityV1 {
    NotRequired,
    Available,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratedImageApiExecutionAvailabilityV1 {
    pub configured: bool,
    pub credential: GeneratedImageCredentialAvailabilityV1,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GeneratedImageExecutionAvailabilityV1 {
    pub plugin_runtime_ready: bool,
    pub local_execution_ready: bool,
    pub api: Option<GeneratedImageApiExecutionAvailabilityV1>,
    pub compute_provider: Option<GpuQueueEligibilityV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedImageExecutionPolicyV1 {
    pub target_order: Vec<GeneratedImageExecutionTargetV1>,
}

impl Default for GeneratedImageExecutionPolicyV1 {
    fn default() -> Self {
        Self {
            target_order: vec![
                GeneratedImageExecutionTargetV1::ComputeProvider,
                GeneratedImageExecutionTargetV1::Api,
                GeneratedImageExecutionTargetV1::LocalPlugin,
            ],
        }
    }
}

impl GeneratedImageExecutionPolicyV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.target_order.is_empty() {
            return Err(Error::InvalidContract(
                "generated image execution target_order must not be empty".to_owned(),
            ));
        }

        let mut seen = BTreeSet::new();
        for target in &self.target_order {
            if !seen.insert(*target) {
                return Err(Error::InvalidContract(
                    "generated image execution target_order must not contain duplicates".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GeneratedImageExecutionRejectionCodeV1 {
    PreparationPreflightBlocked,
    ComputeProviderRequired,
    LocalPluginRuntimeUnavailable,
    LocalExecutionUnavailable,
    ApiCapabilityMissing,
    ApiNetworkPermissionMissing,
    ApiAdapterRuntimeUnavailable,
    ApiConfigurationMissing,
    ApiCredentialMissing,
    ComputeProviderNotRequested,
    ComputeResourceUnsupported,
    ComputeEligibilityMissing,
    ComputeEligibilityJobMismatch,
    ComputeProviderNotReady,
    ComputeSelectionMissing,
    ComputeSelectionInvalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedImageExecutionTargetRejectionV1 {
    pub target: GeneratedImageExecutionTargetV1,
    pub code: GeneratedImageExecutionRejectionCodeV1,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedImageExecutionDecisionStatusV1 {
    Ready,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedImageExecutionDecisionV1 {
    pub schema: String,
    pub version: u32,
    pub status: GeneratedImageExecutionDecisionStatusV1,
    pub target: Option<GeneratedImageExecutionTargetV1>,
    #[serde(default)]
    pub preflight_issues: Vec<GeneratedImagePreflightIssueCodeV1>,
    #[serde(default)]
    pub rejections: Vec<GeneratedImageExecutionTargetRejectionV1>,
}

impl GeneratedImageExecutionDecisionV1 {
    pub fn is_ready(&self) -> bool {
        self.status == GeneratedImageExecutionDecisionStatusV1::Ready
    }

    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != GENERATED_IMAGE_EXECUTION_DECISION_SCHEMA_V1 || self.version != 1 {
            return Err(Error::InvalidContract(
                "unsupported generated image execution decision schema/version".to_owned(),
            ));
        }
        match (self.status, self.target) {
            (GeneratedImageExecutionDecisionStatusV1::Ready, Some(_))
            | (GeneratedImageExecutionDecisionStatusV1::Blocked, None) => Ok(()),
            _ => Err(Error::InvalidContract(
                "generated image execution decision status/target is inconsistent".to_owned(),
            )),
        }
    }

    pub fn require_target_v1(&self) -> Result<GeneratedImageExecutionTargetV1> {
        self.validate_v1()?;
        self.target.ok_or_else(|| {
            Error::InvalidContract(
                "generated image execution is blocked; no execution target is ready".to_owned(),
            )
        })
    }

    pub fn has_rejection(
        &self,
        target: GeneratedImageExecutionTargetV1,
        code: GeneratedImageExecutionRejectionCodeV1,
    ) -> bool {
        self.rejections
            .iter()
            .any(|rejection| rejection.target == target && rejection.code == code)
    }
}

pub fn resolve_generated_image_execution_target_v1(
    job_id: &str,
    preparation: &GeneratedImagePreparationV1,
    plugin: &DiscoveredPlugin,
    availability: &GeneratedImageExecutionAvailabilityV1,
    policy: &GeneratedImageExecutionPolicyV1,
) -> Result<GeneratedImageExecutionDecisionV1> {
    require_non_empty("generated image execution job_id", job_id)?;
    policy.validate_v1()?;

    let preflight = preparation.preflight_v1(plugin);
    let preflight_issues = preflight
        .issues
        .iter()
        .map(|issue| issue.code)
        .collect::<Vec<_>>();

    if !preflight.is_ready() {
        let issue_summary = preflight_issues
            .iter()
            .map(|code| format!("{code:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let rejections = policy
            .target_order
            .iter()
            .copied()
            .map(|target| GeneratedImageExecutionTargetRejectionV1 {
                target,
                code: GeneratedImageExecutionRejectionCodeV1::PreparationPreflightBlocked,
                message: format!(
                    "Generated image preparation preflight is blocked: {issue_summary}."
                ),
            })
            .collect();

        return decision(
            GeneratedImageExecutionDecisionStatusV1::Blocked,
            None,
            preflight_issues,
            rejections,
        );
    }

    let mut rejections = Vec::new();
    for target in &policy.target_order {
        match evaluate_target(*target, job_id, preparation, plugin, availability) {
            Ok(()) => {
                return decision(
                    GeneratedImageExecutionDecisionStatusV1::Ready,
                    Some(*target),
                    preflight_issues,
                    rejections,
                );
            }
            Err(rejection) => rejections.push(rejection),
        }
    }

    decision(
        GeneratedImageExecutionDecisionStatusV1::Blocked,
        None,
        preflight_issues,
        rejections,
    )
}

fn evaluate_target(
    target: GeneratedImageExecutionTargetV1,
    job_id: &str,
    preparation: &GeneratedImagePreparationV1,
    plugin: &DiscoveredPlugin,
    availability: &GeneratedImageExecutionAvailabilityV1,
) -> std::result::Result<(), GeneratedImageExecutionTargetRejectionV1> {
    match target {
        GeneratedImageExecutionTargetV1::LocalPlugin => {
            if preparation.gpu_execution_requested {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::ComputeProviderRequired,
                    "GPU execution was explicitly requested, so local execution cannot be selected.",
                ));
            }
            if !availability.plugin_runtime_ready {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::LocalPluginRuntimeUnavailable,
                    "The local plugin runtime is not ready.",
                ));
            }
            if !availability.local_execution_ready {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::LocalExecutionUnavailable,
                    "The plugin has no ready local execution configuration/runtime.",
                ));
            }
            Ok(())
        }
        GeneratedImageExecutionTargetV1::Api => {
            if preparation.gpu_execution_requested {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::ComputeProviderRequired,
                    "GPU execution was explicitly requested, so API execution cannot be selected.",
                ));
            }
            if !plugin
                .manifest
                .capabilities
                .iter()
                .any(|capability| capability == GENERATED_IMAGE_API_EXECUTION_CAPABILITY_V1)
            {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::ApiCapabilityMissing,
                    "The plugin does not declare the additive api_execution capability.",
                ));
            }
            if plugin.manifest.permissions.network.is_empty() {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::ApiNetworkPermissionMissing,
                    "API-backed execution requires an explicit plugin network permission.",
                ));
            }
            if !availability.plugin_runtime_ready {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::ApiAdapterRuntimeUnavailable,
                    "The local plugin adapter runtime is not ready.",
                ));
            }
            let Some(api) = availability.api else {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::ApiConfigurationMissing,
                    "API-backed execution has no resolved machine-local configuration.",
                ));
            };
            if !api.configured {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::ApiConfigurationMissing,
                    "API-backed execution configuration is incomplete.",
                ));
            }
            if api.credential == GeneratedImageCredentialAvailabilityV1::Missing {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::ApiCredentialMissing,
                    "API-backed execution requires a machine-local credential that is not available.",
                ));
            }
            Ok(())
        }
        GeneratedImageExecutionTargetV1::ComputeProvider => {
            if !preparation.gpu_execution_requested {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::ComputeProviderNotRequested,
                    "ComputeProvider execution was not requested for this generated image.",
                ));
            }

            match plugin.manifest.resources.as_ref() {
                Some(requirements)
                    if matches!(
                        requirements.gpu,
                        ResourceRequirement::Optional | ResourceRequirement::Required
                    ) => {}
                _ => {
                    return Err(rejection(
                        target,
                        GeneratedImageExecutionRejectionCodeV1::ComputeResourceUnsupported,
                        "The plugin does not declare GPU-capable compute resources.",
                    ));
                }
            }

            let Some(eligibility) = availability.compute_provider.as_ref() else {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::ComputeEligibilityMissing,
                    "Canonical GPU queue eligibility has not been evaluated.",
                ));
            };
            if eligibility.job_id != job_id {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::ComputeEligibilityJobMismatch,
                    "GPU queue eligibility belongs to a different logical job.",
                ));
            }
            if eligibility.status != GpuQueueEligibilityStatusV1::GpuReady {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::ComputeProviderNotReady,
                    "Canonical GPU queue eligibility is not GPU_READY.",
                ));
            }

            let Some(selection) = eligibility.selection.as_ref() else {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::ComputeSelectionMissing,
                    "GPU_READY eligibility is missing its canonical device selection.",
                ));
            };
            if selection.provider_id.trim().is_empty()
                || selection.session_id.trim().is_empty()
                || selection.device_id.trim().is_empty()
                || selection.parallelism_group.trim().is_empty()
                || preparation.provider_id.as_deref() != Some(selection.provider_id.as_str())
            {
                return Err(rejection(
                    target,
                    GeneratedImageExecutionRejectionCodeV1::ComputeSelectionInvalid,
                    "Canonical ComputeProvider selection is incomplete or does not match preparation.",
                ));
            }
            Ok(())
        }
    }
}

fn decision(
    status: GeneratedImageExecutionDecisionStatusV1,
    target: Option<GeneratedImageExecutionTargetV1>,
    preflight_issues: Vec<GeneratedImagePreflightIssueCodeV1>,
    rejections: Vec<GeneratedImageExecutionTargetRejectionV1>,
) -> Result<GeneratedImageExecutionDecisionV1> {
    let decision = GeneratedImageExecutionDecisionV1 {
        schema: GENERATED_IMAGE_EXECUTION_DECISION_SCHEMA_V1.to_owned(),
        version: 1,
        status,
        target,
        preflight_issues,
        rejections,
    };
    decision.validate_v1()?;
    Ok(decision)
}

fn rejection(
    target: GeneratedImageExecutionTargetV1,
    code: GeneratedImageExecutionRejectionCodeV1,
    message: impl Into<String>,
) -> GeneratedImageExecutionTargetRejectionV1 {
    GeneratedImageExecutionTargetRejectionV1 {
        target,
        code,
        message: message.into(),
    }
}

fn require_non_empty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidContract(format!("{label} must not be empty")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use super::*;
    use crate::{
        ComputeDeviceSelectionV1, ComputeRequirements, GeneratedImageRequestV1,
        GeneratedImageResolutionV1, GeneratedImageStyleV1, LogicalUri, PluginEntrypoint,
        PluginManifest, PluginPermissions, SceneIntentV1, SCENE_INTENT_SCHEMA,
        SCENE_INTENT_SCHEMA_VERSION,
    };

    fn scene() -> SceneIntentV1 {
        SceneIntentV1 {
            schema: SCENE_INTENT_SCHEMA.to_owned(),
            schema_version: SCENE_INTENT_SCHEMA_VERSION,
            id: "SC-P2A".to_owned(),
            segment_id: "S-P2A".to_owned(),
            narration: "A quiet desk lamp illuminates an unfinished sketch.".to_owned(),
            purpose: "Show focused creative work.".to_owned(),
            scene_type: "conceptual".to_owned(),
            emotion_before: Some("uncertain".to_owned()),
            emotion_after: Some("focused".to_owned()),
            duration_hint: Some(6.0),
            visual_ideas: vec!["warm desk lamp and sketchbook".to_owned()],
            search_queries: vec!["warm desk lamp sketchbook".to_owned()],
            avoid: vec!["logos".to_owned()],
            continuity: BTreeMap::new(),
            aspect_ratio: "16:9".to_owned(),
        }
    }

    fn plugin(api_execution: bool) -> DiscoveredPlugin {
        let mut capabilities = vec![
            "generated_still".to_owned(),
            "visual_generate".to_owned(),
            "deterministic_seed".to_owned(),
        ];
        let mut network = Vec::new();
        if api_execution {
            capabilities.push(GENERATED_IMAGE_API_EXECUTION_CAPABILITY_V1.to_owned());
            network.push("api.example.invalid".to_owned());
        }

        DiscoveredPlugin {
            directory: PathBuf::from("plugins/generated-image-fixture"),
            manifest_path: PathBuf::from("plugins/generated-image-fixture/plugin.yaml"),
            manifest: PluginManifest {
                schema: "omnicreator.plugin-manifest".to_owned(),
                schema_version: 1,
                id: "generated-image-fixture".to_owned(),
                name: "Generated Image Fixture".to_owned(),
                version: "1.0.0".to_owned(),
                api_version: 1,
                types: vec!["visual".to_owned()],
                entrypoint: PluginEntrypoint {
                    command: "python3".to_owned(),
                    args: vec!["plugin.py".to_owned()],
                },
                capabilities,
                scene_types: vec!["conceptual".to_owned()],
                permissions: PluginPermissions {
                    filesystem: vec!["job-workspace".to_owned()],
                    network,
                },
                settings: None,
                resources: Some(ComputeRequirements {
                    gpu: ResourceRequirement::Optional,
                    min_vram_mb: Some(1024),
                    model_group: Some("generated-image-fixture-v1".to_owned()),
                    parallelizable: true,
                    cost_metric: Some("megapixels".to_owned()),
                }),
            },
        }
    }

    fn preparation(gpu_execution_requested: bool) -> GeneratedImagePreparationV1 {
        let request = GeneratedImageRequestV1::from_scene_v1(
            scene(),
            GeneratedImageStyleV1 {
                preset: "cinematic-warm".to_owned(),
                description: None,
            },
            GeneratedImageResolutionV1 {
                width: 1280,
                height: 720,
            },
            Some(42),
            BTreeMap::new(),
        )
        .unwrap();

        GeneratedImagePreparationV1 {
            request,
            output_uri: Some(LogicalUri::parse("project://visual/SC-P2A.png").unwrap()),
            provider_id: gpu_execution_requested.then(|| "compute-fixture".to_owned()),
            model_id: Some("fixture-model".to_owned()),
            model_version: Some("1".to_owned()),
            approval_required: false,
            approval_complete: true,
            production_lock_required: false,
            gpu_execution_requested,
        }
    }

    fn gpu_ready(job_id: &str) -> GpuQueueEligibilityV1 {
        GpuQueueEligibilityV1 {
            job_id: job_id.to_owned(),
            status: GpuQueueEligibilityStatusV1::GpuReady,
            reasons: Vec::new(),
            selection: Some(ComputeDeviceSelectionV1 {
                provider_id: "compute-fixture".to_owned(),
                session_id: "session-fixture".to_owned(),
                device_id: "gpu0".to_owned(),
                parallelizable: true,
                parallelism_group: "generated-image-fixture-v1".to_owned(),
            }),
        }
    }

    #[test]
    fn legacy_generated_plugin_resolves_to_local_without_manifest_breaking_change() {
        let decision = resolve_generated_image_execution_target_v1(
            "job-local",
            &preparation(false),
            &plugin(false),
            &GeneratedImageExecutionAvailabilityV1 {
                plugin_runtime_ready: true,
                local_execution_ready: true,
                ..GeneratedImageExecutionAvailabilityV1::default()
            },
            &GeneratedImageExecutionPolicyV1::default(),
        )
        .unwrap();

        assert_eq!(
            decision.require_target_v1().unwrap(),
            GeneratedImageExecutionTargetV1::LocalPlugin
        );
        assert!(decision.has_rejection(
            GeneratedImageExecutionTargetV1::Api,
            GeneratedImageExecutionRejectionCodeV1::ApiCapabilityMissing
        ));
    }

    #[test]
    fn api_backed_plugin_resolves_from_capability_and_machine_local_readiness() {
        let decision = resolve_generated_image_execution_target_v1(
            "job-api",
            &preparation(false),
            &plugin(true),
            &GeneratedImageExecutionAvailabilityV1 {
                plugin_runtime_ready: true,
                local_execution_ready: false,
                api: Some(GeneratedImageApiExecutionAvailabilityV1 {
                    configured: true,
                    credential: GeneratedImageCredentialAvailabilityV1::Available,
                }),
                compute_provider: None,
            },
            &GeneratedImageExecutionPolicyV1::default(),
        )
        .unwrap();

        assert_eq!(
            decision.require_target_v1().unwrap(),
            GeneratedImageExecutionTargetV1::Api
        );
    }

    #[test]
    fn missing_api_credential_blocks_before_execution() {
        let decision = resolve_generated_image_execution_target_v1(
            "job-api",
            &preparation(false),
            &plugin(true),
            &GeneratedImageExecutionAvailabilityV1 {
                plugin_runtime_ready: true,
                local_execution_ready: false,
                api: Some(GeneratedImageApiExecutionAvailabilityV1 {
                    configured: true,
                    credential: GeneratedImageCredentialAvailabilityV1::Missing,
                }),
                compute_provider: None,
            },
            &GeneratedImageExecutionPolicyV1 {
                target_order: vec![GeneratedImageExecutionTargetV1::Api],
            },
        )
        .unwrap();

        assert!(!decision.is_ready());
        assert!(decision.has_rejection(
            GeneratedImageExecutionTargetV1::Api,
            GeneratedImageExecutionRejectionCodeV1::ApiCredentialMissing
        ));
        assert!(decision.require_target_v1().is_err());
    }

    #[test]
    fn compute_provider_target_reuses_canonical_gpu_ready_selection() {
        let decision = resolve_generated_image_execution_target_v1(
            "job-gpu",
            &preparation(true),
            &plugin(false),
            &GeneratedImageExecutionAvailabilityV1 {
                plugin_runtime_ready: false,
                local_execution_ready: false,
                api: None,
                compute_provider: Some(gpu_ready("job-gpu")),
            },
            &GeneratedImageExecutionPolicyV1::default(),
        )
        .unwrap();

        assert_eq!(
            decision.require_target_v1().unwrap(),
            GeneratedImageExecutionTargetV1::ComputeProvider
        );

        let encoded = serde_json::to_string(&decision).unwrap();
        assert!(!encoded.contains("compute-fixture"));
        assert!(!encoded.contains("session-fixture"));
        assert!(!encoded.contains("gpu0"));
    }

    #[test]
    fn non_ready_compute_provider_does_not_fall_back_when_gpu_was_requested() {
        let mut eligibility = gpu_ready("job-gpu");
        eligibility.status = GpuQueueEligibilityStatusV1::NotReady;
        eligibility.selection = None;

        let decision = resolve_generated_image_execution_target_v1(
            "job-gpu",
            &preparation(true),
            &plugin(true),
            &GeneratedImageExecutionAvailabilityV1 {
                plugin_runtime_ready: true,
                local_execution_ready: true,
                api: Some(GeneratedImageApiExecutionAvailabilityV1 {
                    configured: true,
                    credential: GeneratedImageCredentialAvailabilityV1::Available,
                }),
                compute_provider: Some(eligibility),
            },
            &GeneratedImageExecutionPolicyV1::default(),
        )
        .unwrap();

        assert!(!decision.is_ready());
        assert!(decision.has_rejection(
            GeneratedImageExecutionTargetV1::ComputeProvider,
            GeneratedImageExecutionRejectionCodeV1::ComputeProviderNotReady
        ));
        assert!(decision.has_rejection(
            GeneratedImageExecutionTargetV1::Api,
            GeneratedImageExecutionRejectionCodeV1::ComputeProviderRequired
        ));
        assert!(decision.has_rejection(
            GeneratedImageExecutionTargetV1::LocalPlugin,
            GeneratedImageExecutionRejectionCodeV1::ComputeProviderRequired
        ));
    }

    #[test]
    fn existing_generated_image_preflight_blocks_target_resolution_first() {
        let mut blocked = preparation(false);
        blocked.output_uri = None;

        let decision = resolve_generated_image_execution_target_v1(
            "job-blocked",
            &blocked,
            &plugin(false),
            &GeneratedImageExecutionAvailabilityV1 {
                plugin_runtime_ready: true,
                local_execution_ready: true,
                ..GeneratedImageExecutionAvailabilityV1::default()
            },
            &GeneratedImageExecutionPolicyV1::default(),
        )
        .unwrap();

        assert!(!decision.is_ready());
        assert!(decision
            .preflight_issues
            .contains(&GeneratedImagePreflightIssueCodeV1::OutputMissing));
        assert!(decision.rejections.iter().all(|rejection| {
            rejection.code == GeneratedImageExecutionRejectionCodeV1::PreparationPreflightBlocked
        }));
    }

    #[test]
    fn target_policy_is_deterministic_and_rejects_duplicates() {
        let duplicate = GeneratedImageExecutionPolicyV1 {
            target_order: vec![
                GeneratedImageExecutionTargetV1::Api,
                GeneratedImageExecutionTargetV1::Api,
            ],
        };
        assert!(duplicate.validate_v1().is_err());

        let api_first = GeneratedImageExecutionPolicyV1 {
            target_order: vec![
                GeneratedImageExecutionTargetV1::Api,
                GeneratedImageExecutionTargetV1::LocalPlugin,
            ],
        };
        let decision = resolve_generated_image_execution_target_v1(
            "job-order",
            &preparation(false),
            &plugin(true),
            &GeneratedImageExecutionAvailabilityV1 {
                plugin_runtime_ready: true,
                local_execution_ready: true,
                api: Some(GeneratedImageApiExecutionAvailabilityV1 {
                    configured: true,
                    credential: GeneratedImageCredentialAvailabilityV1::NotRequired,
                }),
                compute_provider: None,
            },
            &api_first,
        )
        .unwrap();

        assert_eq!(
            decision.require_target_v1().unwrap(),
            GeneratedImageExecutionTargetV1::Api
        );
    }
}
