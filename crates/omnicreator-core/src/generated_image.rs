use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    artifact_store::PluginOutputPromotion, deterministic_input_hash, fs_util::sha256_file,
    Artifact, ArtifactStore, ComputeRequirements, DiscoveredPlugin, Error, GpuJobPreparationV1,
    LogicalUri, PluginJobWorkspace, PluginProcess, PluginProcessOptions, PluginResponse, Result,
    SceneIntentV1, StateStore, VisualRouteV1, VisualRoutingDecisionV1, VisualUseCaseV1,
};

pub const GENERATED_IMAGE_REQUEST_SCHEMA_V1: &str = "omnicreator.generated-image-request";
pub const GENERATED_IMAGE_OPERATION_V1: &str = "visual.generate";
pub const GENERATED_STILL_CAPABILITY_V1: &str = "generated_still";
pub const VISUAL_GENERATE_CAPABILITY_V1: &str = "visual_generate";
pub const DETERMINISTIC_SEED_CAPABILITY_V1: &str = "deterministic_seed";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedImageStyleV1 {
    pub preset: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedImageResolutionV1 {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneratedImageRequestV1 {
    pub schema: String,
    pub version: u32,
    pub scene: SceneIntentV1,
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub style: GeneratedImageStyleV1,
    pub resolution: GeneratedImageResolutionV1,
    pub aspect_ratio: String,
    pub seed: Option<u64>,
    #[serde(default)]
    pub settings: BTreeMap<String, Value>,
    pub prompt_sha256: String,
    pub settings_fingerprint: String,
}

impl GeneratedImageRequestV1 {
    pub fn from_scene_v1(
        scene: SceneIntentV1,
        style: GeneratedImageStyleV1,
        resolution: GeneratedImageResolutionV1,
        seed: Option<u64>,
        settings: BTreeMap<String, Value>,
    ) -> Result<Self> {
        scene.validate_v1()?;
        validate_style_v1(&style)?;
        validate_resolution_v1(&resolution)?;
        validate_portable_value_v1(
            &serde_json::to_value(&settings)?,
            "generated image settings",
        )?;

        let prompt = build_generated_image_prompt_v1(&scene, &style)?;
        let negative_prompt = canonical_list_v1(&scene.avoid);
        let prompt_sha256 = prompt_hash_v1(&prompt);
        let settings_fingerprint =
            settings_fingerprint_v1(&style, &resolution, &scene.aspect_ratio, seed, &settings)?;

        let request = Self {
            schema: GENERATED_IMAGE_REQUEST_SCHEMA_V1.to_owned(),
            version: 1,
            aspect_ratio: scene.aspect_ratio.clone(),
            scene,
            prompt,
            negative_prompt,
            style,
            resolution,
            seed,
            settings,
            prompt_sha256,
            settings_fingerprint,
        };
        request.validate_v1()?;
        Ok(request)
    }

    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != GENERATED_IMAGE_REQUEST_SCHEMA_V1 || self.version != 1 {
            return Err(Error::InvalidContract(
                "unsupported generated image request schema/version".to_owned(),
            ));
        }
        self.scene.validate_v1()?;
        require_non_empty("generated image prompt", &self.prompt)?;
        validate_style_v1(&self.style)?;
        validate_resolution_v1(&self.resolution)?;
        require_non_empty("generated image aspect_ratio", &self.aspect_ratio)?;
        if self.aspect_ratio != self.scene.aspect_ratio {
            return Err(Error::InvalidContract(
                "generated image aspect_ratio must match SceneIntent".to_owned(),
            ));
        }
        validate_portable_value_v1(
            &serde_json::to_value(&self.settings)?,
            "generated image settings",
        )?;

        let expected_prompt_hash = prompt_hash_v1(&self.prompt);
        if self.prompt_sha256 != expected_prompt_hash {
            return Err(Error::InvalidContract(
                "generated image prompt_sha256 is stale".to_owned(),
            ));
        }
        let expected_settings = settings_fingerprint_v1(
            &self.style,
            &self.resolution,
            &self.aspect_ratio,
            self.seed,
            &self.settings,
        )?;
        if self.settings_fingerprint != expected_settings {
            return Err(Error::InvalidContract(
                "generated image settings_fingerprint is stale".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn input_hash_v1(&self) -> Result<String> {
        self.validate_v1()?;
        let encoded = serde_json::to_vec(self)?;
        Ok(deterministic_input_hash(&[
            b"generated-image-request-v1",
            encoded.as_slice(),
        ]))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GeneratedImagePreflightIssueCodeV1 {
    SceneInvalid,
    PromptMissing,
    StyleMissing,
    ResolutionInvalid,
    UnsafeSettings,
    PluginInvalid,
    VisualTypeMissing,
    GenerateCapabilityMissing,
    SeedUnsupported,
    WorkspacePermissionMissing,
    ResourceDeclarationMissing,
    ResourceDeclarationInvalid,
    ModelGroupMissing,
    ModelMissing,
    ModelVersionMissing,
    ProviderMissing,
    OutputMissing,
    OutputInvalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedImagePreflightIssueV1 {
    pub code: GeneratedImagePreflightIssueCodeV1,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GeneratedImagePreflightStatusV1 {
    Ready,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedImagePreflightV1 {
    pub status: GeneratedImagePreflightStatusV1,
    #[serde(default)]
    pub issues: Vec<GeneratedImagePreflightIssueV1>,
}

impl GeneratedImagePreflightV1 {
    pub fn is_ready(&self) -> bool {
        self.status == GeneratedImagePreflightStatusV1::Ready
    }

    pub fn has(&self, code: GeneratedImagePreflightIssueCodeV1) -> bool {
        self.issues.iter().any(|issue| issue.code == code)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneratedImagePreparationV1 {
    pub request: GeneratedImageRequestV1,
    pub output_uri: Option<LogicalUri>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub model_version: Option<String>,
    pub approval_required: bool,
    pub approval_complete: bool,
    pub production_lock_required: bool,
    pub gpu_execution_requested: bool,
}

impl GeneratedImagePreparationV1 {
    pub fn preflight_v1(&self, plugin: &DiscoveredPlugin) -> GeneratedImagePreflightV1 {
        let mut issues = Vec::new();

        if self.request.scene.validate_v1().is_err() {
            push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::SceneInvalid,
                "SceneIntent is invalid or incomplete.",
            );
        }
        if self.request.prompt.trim().is_empty() {
            push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::PromptMissing,
                "Generated image prompt is empty.",
            );
        }
        if validate_style_v1(&self.request.style).is_err() {
            push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::StyleMissing,
                "Generated image style/preset is missing or invalid.",
            );
        }
        if validate_resolution_v1(&self.request.resolution).is_err() {
            push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::ResolutionInvalid,
                "Generated image resolution must be positive and within the v1 safety bound.",
            );
        }
        if validate_portable_value_v1(
            &serde_json::to_value(&self.request.settings).unwrap_or(Value::Null),
            "generated image settings",
        )
        .is_err()
        {
            push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::UnsafeSettings,
                "Generated image settings contain secret-like fields or machine-specific absolute paths.",
            );
        }

        if plugin.manifest.validate_v1().is_err() {
            push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::PluginInvalid,
                "Selected generated image plugin manifest is invalid.",
            );
        }
        if !plugin.manifest.types.iter().any(|value| value == "visual") {
            push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::VisualTypeMissing,
                "Selected plugin does not declare the visual plugin type.",
            );
        }
        if !plugin
            .manifest
            .capabilities
            .iter()
            .any(|value| value == GENERATED_STILL_CAPABILITY_V1)
            || !plugin
                .manifest
                .capabilities
                .iter()
                .any(|value| value == VISUAL_GENERATE_CAPABILITY_V1)
        {
            push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::GenerateCapabilityMissing,
                "Selected plugin does not declare generated-still visual.generate capability.",
            );
        }
        if self.request.seed.is_some()
            && !plugin
                .manifest
                .capabilities
                .iter()
                .any(|value| value == DETERMINISTIC_SEED_CAPABILITY_V1)
        {
            push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::SeedUnsupported,
                "A deterministic seed was requested but the plugin does not declare deterministic seed support.",
            );
        }
        if !plugin
            .manifest
            .permissions
            .filesystem
            .iter()
            .any(|value| value == "job-workspace")
        {
            push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::WorkspacePermissionMissing,
                "Generated image plugin must be restricted to the granted job workspace.",
            );
        }

        match plugin.manifest.resources.as_ref() {
            None => push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::ResourceDeclarationMissing,
                "Generated image plugin must declare provider-neutral compute resources.",
            ),
            Some(requirements) => {
                if requirements.validate_scheduling_v1().is_err() {
                    push_issue(
                        &mut issues,
                        GeneratedImagePreflightIssueCodeV1::ResourceDeclarationInvalid,
                        "Generated image compute resource declaration is invalid.",
                    );
                }
                if requirements
                    .model_group
                    .as_deref()
                    .map_or(true, |value| value.trim().is_empty())
                {
                    push_issue(
                        &mut issues,
                        GeneratedImagePreflightIssueCodeV1::ModelGroupMissing,
                        "Generated image resource declaration must expose a model_group for batch planning.",
                    );
                }
            }
        }

        if normalized_option(self.model_id.as_deref()).is_none() {
            push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::ModelMissing,
                "Generated image model identity is not resolved.",
            );
        }
        if normalized_option(self.model_version.as_deref()).is_none() {
            push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::ModelVersionMissing,
                "Generated image model version is not pinned.",
            );
        }
        if self.gpu_execution_requested && normalized_option(self.provider_id.as_deref()).is_none()
        {
            push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::ProviderMissing,
                "GPU execution requires a selected ComputeProvider.",
            );
        }

        match self.output_uri.as_ref() {
            None => push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::OutputMissing,
                "Generated image logical output destination is unresolved.",
            ),
            Some(LogicalUri::Artifact(_)) => push_issue(
                &mut issues,
                GeneratedImagePreflightIssueCodeV1::OutputInvalid,
                "artifact:// cannot be used as a physical generated image promotion target.",
            ),
            Some(_) => {}
        }

        issues.sort_by_key(|issue| issue.code);
        GeneratedImagePreflightV1 {
            status: if issues.is_empty() {
                GeneratedImagePreflightStatusV1::Ready
            } else {
                GeneratedImagePreflightStatusV1::Blocked
            },
            issues,
        }
    }

    pub fn to_gpu_job_preparation_v1(
        &self,
        job_id: &str,
        plugin: &DiscoveredPlugin,
    ) -> Result<GpuJobPreparationV1> {
        require_non_empty("generated image job_id", job_id)?;
        let requirements = plugin.manifest.resources.clone().ok_or_else(|| {
            Error::InvalidContract(
                "generated image plugin has no compute resource declaration".to_owned(),
            )
        })?;
        requirements.validate_scheduling_v1()?;
        let preflight = self.preflight_v1(plugin);
        let input_resolved = self.request.validate_v1().is_ok();

        Ok(GpuJobPreparationV1 {
            job_id: job_id.to_owned(),
            input_resolved,
            input_immutable: input_resolved,
            plugin_id: Some(plugin.manifest.id.clone()),
            provider_id: normalized_option(self.provider_id.as_deref()),
            model_id: normalized_option(self.model_id.as_deref()),
            model_version: normalized_option(self.model_version.as_deref()),
            settings_fingerprint: Some(self.request.settings_fingerprint.clone()),
            output_uri: self.output_uri.clone(),
            approval_required: self.approval_required,
            approval_complete: self.approval_complete,
            production_lock_required: self.production_lock_required,
            preflight_required: true,
            preflight_complete: preflight.is_ready(),
            gpu_execution_requested: self.gpu_execution_requested,
            requirements,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneratedImagePluginResultV1 {
    pub relative_output: String,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
    pub model_id: String,
    pub model_version: String,
    pub seed: Option<u64>,
    pub prompt_sha256: String,
    pub settings_fingerprint: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
    #[serde(default)]
    pub provenance: BTreeMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct GeneratedImageExecutionV1 {
    pub attempt_id: String,
    pub artifact: Artifact,
    pub plugin_result: GeneratedImagePluginResultV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneratedImageExecutionContextV1 {
    pub use_case: VisualUseCaseV1,
    pub routing: Option<VisualRoutingDecisionV1>,
}

impl Default for GeneratedImageExecutionContextV1 {
    fn default() -> Self {
        Self {
            use_case: VisualUseCaseV1::SceneVisual,
            routing: None,
        }
    }
}

impl GeneratedImageExecutionContextV1 {
    pub fn validate_for_scene_v1(&self, scene: &SceneIntentV1) -> Result<()> {
        scene.validate_v1()?;

        match self.routing.as_ref() {
            Some(routing) => {
                routing.validate_v1()?;
                if routing.scene_id != scene.id {
                    return Err(Error::InvalidContract(format!(
                        "generated image routing scene {} does not match SceneIntent {}",
                        routing.scene_id, scene.id
                    )));
                }
                if routing.route != VisualRouteV1::GeneratedStill {
                    return Err(Error::InvalidContract(
                        "generated image execution context requires a generated_still route"
                            .to_owned(),
                    ));
                }
                if routing.use_case != self.use_case {
                    return Err(Error::InvalidContract(
                        "generated image execution use_case does not match routing decision"
                            .to_owned(),
                    ));
                }
            }
            None if self.use_case == VisualUseCaseV1::ThumbnailBackground => {
                return Err(Error::InvalidContract(
                    "thumbnail background generation requires an explicit routing decision"
                        .to_owned(),
                ));
            }
            None => {}
        }

        Ok(())
    }
}

pub fn execute_generated_image_plugin_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    plugin: &DiscoveredPlugin,
    runtime_root: impl AsRef<Path>,
    job_id: &str,
    preparation: &GeneratedImagePreparationV1,
    process_options: PluginProcessOptions,
) -> Result<GeneratedImageExecutionV1> {
    execute_generated_image_plugin_with_context_v1(
        state_store,
        artifact_store,
        plugin,
        runtime_root,
        job_id,
        preparation,
        &GeneratedImageExecutionContextV1::default(),
        process_options,
    )
}

pub fn execute_generated_image_plugin_with_context_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    plugin: &DiscoveredPlugin,
    runtime_root: impl AsRef<Path>,
    job_id: &str,
    preparation: &GeneratedImagePreparationV1,
    context: &GeneratedImageExecutionContextV1,
    process_options: PluginProcessOptions,
) -> Result<GeneratedImageExecutionV1> {
    context.validate_for_scene_v1(&preparation.request.scene)?;
    let preflight = preparation.preflight_v1(plugin);
    if !preflight.is_ready() {
        let codes = preflight
            .issues
            .iter()
            .map(|issue| format!("{:?}", issue.code))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Error::InvalidContract(format!(
            "generated image preflight blocked before execution: {codes}"
        )));
    }
    preparation.request.validate_v1()?;

    let expected_input_hash = preparation.request.input_hash_v1()?;
    let job = state_store.get_job(job_id)?;
    if job.input_hash != expected_input_hash {
        return Err(Error::InvalidJobState(format!(
            "generated image job {} input hash does not match immutable request",
            job.job_id
        )));
    }
    if job.unit != preparation.request.scene.id {
        return Err(Error::InvalidJobState(format!(
            "generated image job {} unit {} does not match SceneIntent {}",
            job.job_id, job.unit, preparation.request.scene.id
        )));
    }

    let workspace = PluginJobWorkspace::create(runtime_root, job_id)?;
    let process = PluginProcess::spawn(plugin, process_options)?;
    let initialize = process.initialize(workspace.initialization_context(plugin)?)?;
    response_result_v1(plugin, initialize.response, "plugin.initialize")?;

    let capabilities = process.capabilities()?;
    let capabilities = response_result_v1(plugin, capabilities.response, "plugin.capabilities")?;
    validate_runtime_capabilities_v1(&capabilities)?;

    let attempt =
        state_store.start_attempt(job_id, Some(&format!("plugin:{}", plugin.manifest.id)))?;
    let call = match process.execute(
        GENERATED_IMAGE_OPERATION_V1,
        serde_json::to_value(&preparation.request)?,
    ) {
        Ok(call) => call,
        Err(error) => {
            let _ = state_store
                .finish_attempt_failure(&attempt.attempt_id, "LOCAL_RUNTIME_CONTEXT_ERROR");
            return Err(error);
        }
    };

    let result_value = match call.response {
        PluginResponse::Success { result, .. } => result,
        PluginResponse::Failure { error, .. } => {
            let state_code = if error.retryable {
                retryable_state_code_v1(&error.code)
            } else {
                error.code.as_str()
            };
            let _ = state_store.finish_attempt_failure(&attempt.attempt_id, state_code);
            return Err(Error::PluginProtocol {
                plugin: plugin.manifest.id.clone(),
                message: format!("{}: {}", error.code, error.message),
            });
        }
    };

    let plugin_result: GeneratedImagePluginResultV1 = match serde_json::from_value(result_value) {
        Ok(result) => result,
        Err(error) => {
            let _ =
                state_store.finish_attempt_failure(&attempt.attempt_id, "INVALID_PLUGIN_OUTPUT");
            return Err(Error::InvalidArtifact(format!(
                "generated image plugin returned invalid result metadata: {error}"
            )));
        }
    };

    if let Err(error) = validate_plugin_result_v1(preparation, plugin, &plugin_result) {
        let _ = state_store.finish_attempt_failure(&attempt.attempt_id, "INVALID_PLUGIN_OUTPUT");
        return Err(error);
    }

    let verified = match workspace.verify_output_file(&plugin_result.relative_output) {
        Ok(verified) => verified,
        Err(error) => {
            let _ =
                state_store.finish_attempt_failure(&attempt.attempt_id, "INVALID_PLUGIN_OUTPUT");
            return Err(error);
        }
    };
    let (actual_sha256, _) = sha256_file(verified.path())?;
    if actual_sha256 != plugin_result.sha256 {
        let _ = state_store.finish_attempt_failure(&attempt.attempt_id, "INVALID_PLUGIN_OUTPUT");
        return Err(Error::InvalidArtifact(
            "generated image plugin sha256 does not match core-computed workspace hash".to_owned(),
        ));
    }

    let metadata =
        generated_image_artifact_metadata_v1(preparation, plugin, &plugin_result, context)?;
    let output_uri = preparation.output_uri.clone().ok_or_else(|| {
        Error::InvalidContract("generated image output URI is missing".to_owned())
    })?;

    let artifact = match artifact_store.promote_plugin_output_for_attempt(
        state_store,
        &attempt.attempt_id,
        job_id,
        &workspace,
        PluginOutputPromotion {
            relative_output: plugin_result.relative_output.clone(),
            target_uri: output_uri,
            artifact_type: "image".to_owned(),
            metadata,
        },
        &plugin_result.sha256,
    ) {
        Ok(artifact) => artifact,
        Err(error) => {
            let _ = state_store
                .finish_attempt_failure(&attempt.attempt_id, "LOCAL_RUNTIME_CONTEXT_ERROR");
            return Err(error);
        }
    };

    let _ = process.shutdown();
    Ok(GeneratedImageExecutionV1 {
        attempt_id: attempt.attempt_id,
        artifact,
        plugin_result,
    })
}

pub fn build_generated_image_prompt_v1(
    scene: &SceneIntentV1,
    style: &GeneratedImageStyleV1,
) -> Result<String> {
    scene.validate_v1()?;
    validate_style_v1(style)?;

    let mut lines = vec![
        format!("Purpose: {}", normalize_text_v1(&scene.purpose)),
        format!("Scene type: {}", normalize_text_v1(&scene.scene_type)),
        format!("Narration: {}", normalize_text_v1(&scene.narration)),
    ];

    if let Some(ideas) = canonical_list_v1(&scene.visual_ideas) {
        lines.push(format!("Visual direction: {ideas}"));
    }
    let before = normalized_option(scene.emotion_before.as_deref());
    let after = normalized_option(scene.emotion_after.as_deref());
    if before.is_some() || after.is_some() {
        lines.push(format!(
            "Emotional arc: {} -> {}",
            before.as_deref().unwrap_or("unspecified"),
            after.as_deref().unwrap_or("unspecified")
        ));
    }
    lines.push(format!(
        "Style preset: {}",
        normalize_text_v1(&style.preset)
    ));
    if let Some(description) = normalized_option(style.description.as_deref()) {
        lines.push(format!("Style direction: {description}"));
    }
    lines.push(format!(
        "Aspect ratio: {}",
        normalize_text_v1(&scene.aspect_ratio)
    ));

    Ok(lines.join("\n"))
}

fn validate_plugin_result_v1(
    preparation: &GeneratedImagePreparationV1,
    plugin: &DiscoveredPlugin,
    result: &GeneratedImagePluginResultV1,
) -> Result<()> {
    require_non_empty("generated image relative_output", &result.relative_output)?;
    if !result.mime_type.starts_with("image/") {
        return Err(Error::InvalidArtifact(
            "generated image result mime_type must be image/*".to_owned(),
        ));
    }
    if result.width != preparation.request.resolution.width
        || result.height != preparation.request.resolution.height
    {
        return Err(Error::InvalidArtifact(
            "generated image result dimensions do not match requested resolution".to_owned(),
        ));
    }
    validate_sha256_v1(&result.sha256)?;
    require_non_empty("generated image model_id", &result.model_id)?;
    require_non_empty("generated image model_version", &result.model_version)?;

    if normalized_option(preparation.model_id.as_deref()).as_deref()
        != Some(result.model_id.as_str())
    {
        return Err(Error::InvalidArtifact(
            "generated image result model_id does not match resolved execution model".to_owned(),
        ));
    }
    if normalized_option(preparation.model_version.as_deref()).as_deref()
        != Some(result.model_version.as_str())
    {
        return Err(Error::InvalidArtifact(
            "generated image result model_version does not match resolved execution model"
                .to_owned(),
        ));
    }
    if plugin
        .manifest
        .capabilities
        .iter()
        .any(|value| value == DETERMINISTIC_SEED_CAPABILITY_V1)
        && result.seed != preparation.request.seed
    {
        return Err(Error::InvalidArtifact(
            "generated image result seed does not match deterministic request".to_owned(),
        ));
    }
    if result.prompt_sha256 != preparation.request.prompt_sha256 {
        return Err(Error::InvalidArtifact(
            "generated image result prompt fingerprint does not match request".to_owned(),
        ));
    }
    if result.settings_fingerprint != preparation.request.settings_fingerprint {
        return Err(Error::InvalidArtifact(
            "generated image result settings fingerprint does not match request".to_owned(),
        ));
    }
    validate_portable_value_v1(
        &serde_json::to_value(&result.metadata)?,
        "generated image provider metadata",
    )?;
    validate_portable_value_v1(
        &serde_json::to_value(&result.provenance)?,
        "generated image provenance",
    )
}

fn generated_image_artifact_metadata_v1(
    preparation: &GeneratedImagePreparationV1,
    plugin: &DiscoveredPlugin,
    result: &GeneratedImagePluginResultV1,
    context: &GeneratedImageExecutionContextV1,
) -> Result<Value> {
    context.validate_for_scene_v1(&preparation.request.scene)?;
    let mut metadata = json!({
        "source": "generated",
        "provider": plugin.manifest.id,
        "model": {
            "id": result.model_id,
            "version": result.model_version
        },
        "seed": result.seed,
        "style": preparation.request.style,
        "resolution": preparation.request.resolution,
        "aspect_ratio": preparation.request.aspect_ratio,
        "prompt": preparation.request.prompt,
        "negative_prompt": preparation.request.negative_prompt,
        "prompt_sha256": result.prompt_sha256,
        "settings": preparation.request.settings,
        "settings_fingerprint": result.settings_fingerprint,
        "mime_type": result.mime_type,
        "provider_metadata": result.metadata,
        "provenance": result.provenance,
        "use_case": context.use_case
    });
    if let Some(routing) = context.routing.as_ref() {
        metadata
            .as_object_mut()
            .expect("generated image artifact metadata is an object")
            .insert("visual_routing".to_owned(), serde_json::to_value(routing)?);
    }
    validate_portable_value_v1(&metadata, "generated image artifact metadata")?;
    Ok(metadata)
}

fn response_result_v1(
    plugin: &DiscoveredPlugin,
    response: PluginResponse,
    method: &str,
) -> Result<Value> {
    match response {
        PluginResponse::Success { result, .. } => Ok(result),
        PluginResponse::Failure { error, .. } => Err(Error::PluginProtocol {
            plugin: plugin.manifest.id.clone(),
            message: format!("{method} failed with {}: {}", error.code, error.message),
        }),
    }
}

fn validate_runtime_capabilities_v1(value: &Value) -> Result<()> {
    let operations = value
        .get("operations")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Error::InvalidContract(
                "generated image plugin capabilities must expose operations".to_owned(),
            )
        })?;
    if !operations
        .iter()
        .filter_map(Value::as_str)
        .any(|operation| operation == GENERATED_IMAGE_OPERATION_V1)
    {
        return Err(Error::InvalidContract(
            "generated image plugin runtime does not advertise visual.generate".to_owned(),
        ));
    }
    Ok(())
}

fn validate_style_v1(style: &GeneratedImageStyleV1) -> Result<()> {
    require_non_empty("generated image style preset", &style.preset)?;
    if style
        .description
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(Error::InvalidContract(
            "generated image style description must not be blank when present".to_owned(),
        ));
    }
    Ok(())
}

fn validate_resolution_v1(resolution: &GeneratedImageResolutionV1) -> Result<()> {
    if resolution.width == 0
        || resolution.height == 0
        || resolution.width > 16_384
        || resolution.height > 16_384
    {
        return Err(Error::InvalidContract(
            "generated image resolution must be within 1..=16384 per dimension".to_owned(),
        ));
    }
    Ok(())
}

fn settings_fingerprint_v1(
    style: &GeneratedImageStyleV1,
    resolution: &GeneratedImageResolutionV1,
    aspect_ratio: &str,
    seed: Option<u64>,
    settings: &BTreeMap<String, Value>,
) -> Result<String> {
    let encoded = serde_json::to_vec(&(style, resolution, aspect_ratio, seed, settings))?;
    Ok(deterministic_input_hash(&[
        b"generated-image-settings-v1",
        encoded.as_slice(),
    ]))
}

fn prompt_hash_v1(prompt: &str) -> String {
    deterministic_input_hash(&[b"generated-image-prompt-v1", prompt.as_bytes()])
}

fn canonical_list_v1(values: &[String]) -> Option<String> {
    let mut values = values
        .iter()
        .map(|value| normalize_text_v1(value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    (!values.is_empty()).then(|| values.join("; "))
}

fn normalize_text_v1(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalized_option(value: Option<&str>) -> Option<String> {
    value
        .map(normalize_text_v1)
        .filter(|value| !value.is_empty())
}

fn validate_sha256_v1(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::InvalidArtifact(
            "generated image sha256 must be 64 hexadecimal characters".to_owned(),
        ));
    }
    Ok(())
}

fn validate_portable_value_v1(value: &Value, path: &str) -> Result<()> {
    match value {
        Value::Object(values) => {
            for (key, child) in values {
                if forbidden_secret_key_v1(key) && !child.is_null() {
                    return Err(Error::InvalidContract(format!(
                        "{path}.{key} must not contain a secret value"
                    )));
                }
                validate_portable_value_v1(child, &format!("{path}.{key}"))?;
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                validate_portable_value_v1(child, &format!("{path}[{index}]"))?;
            }
        }
        Value::String(value) if looks_like_absolute_machine_path_v1(value) => {
            return Err(Error::InvalidContract(format!(
                "{path} must not contain durable machine-specific absolute path {value}"
            )));
        }
        _ => {}
    }
    Ok(())
}

fn forbidden_secret_key_v1(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace('-', "_");
    matches!(
        key.as_str(),
        "api_key"
            | "apikey"
            | "secret"
            | "password"
            | "access_token"
            | "refresh_token"
            | "bearer_token"
            | "credential_value"
    )
}

fn looks_like_absolute_machine_path_v1(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.starts_with('/')
        || value.starts_with("\\\\")
        || value.starts_with("file://")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

fn retryable_state_code_v1(code: &str) -> &str {
    match code {
        "NETWORK_TIMEOUT"
        | "WORKER_LOST"
        | "MODEL_LOAD_ERROR"
        | "CUDA_OOM"
        | "RATE_LIMITED"
        | "QUOTA_EXHAUSTED"
        | "QUOTA_TEMPORARY"
        | "PROVIDER_UNAVAILABLE" => code,
        _ => "PROVIDER_UNAVAILABLE",
    }
}

fn push_issue(
    issues: &mut Vec<GeneratedImagePreflightIssueV1>,
    code: GeneratedImagePreflightIssueCodeV1,
    message: impl Into<String>,
) {
    if issues.iter().any(|issue| issue.code == code) {
        return;
    }
    issues.push(GeneratedImagePreflightIssueV1 {
        code,
        message: message.into(),
    });
}

fn require_non_empty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidContract(format!("{label} must not be empty")));
    }
    Ok(())
}

pub fn default_generated_image_compute_requirements_v1(
    model_group: impl Into<String>,
    min_vram_mb: u64,
) -> ComputeRequirements {
    ComputeRequirements {
        gpu: crate::ResourceRequirement::Optional,
        min_vram_mb: Some(min_vram_mb),
        model_group: Some(model_group.into()),
        parallelizable: true,
        cost_metric: Some("megapixels".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SCENE_INTENT_SCHEMA, SCENE_INTENT_SCHEMA_VERSION};

    fn scene() -> SceneIntentV1 {
        SceneIntentV1 {
            schema: SCENE_INTENT_SCHEMA.to_owned(),
            schema_version: SCENE_INTENT_SCHEMA_VERSION,
            id: "SC01".to_owned(),
            segment_id: "S01".to_owned(),
            narration: "A quiet craftsperson repairs a weathered wooden gate.".to_owned(),
            purpose: "Show patient restoration.".to_owned(),
            scene_type: "conceptual".to_owned(),
            emotion_before: Some("worn".to_owned()),
            emotion_after: Some("hopeful".to_owned()),
            duration_hint: Some(5.0),
            visual_ideas: vec![
                "close hands repairing wood".to_owned(),
                "warm dawn light".to_owned(),
            ],
            search_queries: vec!["repairing wooden gate".to_owned()],
            avoid: vec!["logos".to_owned(), "text overlays".to_owned()],
            continuity: BTreeMap::new(),
            aspect_ratio: "16:9".to_owned(),
        }
    }

    #[test]
    fn request_mapping_is_deterministic_and_provider_neutral() {
        let style = GeneratedImageStyleV1 {
            preset: "cinematic-warm".to_owned(),
            description: Some("natural texture, restrained contrast".to_owned()),
        };
        let resolution = GeneratedImageResolutionV1 {
            width: 1280,
            height: 720,
        };
        let first = GeneratedImageRequestV1::from_scene_v1(
            scene(),
            style.clone(),
            resolution.clone(),
            Some(42),
            BTreeMap::new(),
        )
        .unwrap();
        let second = GeneratedImageRequestV1::from_scene_v1(
            scene(),
            style,
            resolution,
            Some(42),
            BTreeMap::new(),
        )
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(
            first.input_hash_v1().unwrap(),
            second.input_hash_v1().unwrap()
        );
        let scene_json = serde_json::to_value(&first.scene).unwrap();
        assert!(scene_json.get("provider").is_none());
        assert!(scene_json.get("model").is_none());
    }

    #[test]
    fn portable_settings_reject_secret_values_and_absolute_paths() {
        let mut secret = BTreeMap::new();
        secret.insert("api_key".to_owned(), json!("do-not-store"));
        assert!(GeneratedImageRequestV1::from_scene_v1(
            scene(),
            GeneratedImageStyleV1 {
                preset: "clean".to_owned(),
                description: None,
            },
            GeneratedImageResolutionV1 {
                width: 1024,
                height: 576,
            },
            Some(1),
            secret,
        )
        .is_err());

        let mut path = BTreeMap::new();
        path.insert("cache_dir".to_owned(), json!("/Users/alice/model-cache"));
        assert!(GeneratedImageRequestV1::from_scene_v1(
            scene(),
            GeneratedImageStyleV1 {
                preset: "clean".to_owned(),
                description: None,
            },
            GeneratedImageResolutionV1 {
                width: 1024,
                height: 576,
            },
            Some(1),
            path,
        )
        .is_err());
    }
}
