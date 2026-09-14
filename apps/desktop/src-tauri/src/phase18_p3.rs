#[derive(Debug, Serialize)]
struct LlmProviderDesktopStatusV1 {
    provider_id: String,
    provider_kind: omnicreator_core::LlmProviderKindV1,
    state: String,
    base_url: String,
    api_key_env: String,
    default_model: String,
    credential_present: bool,
    model_discovery_supported: bool,
    reason_code: Option<String>,
    models: Vec<omnicreator_core::LlmProviderModelV1>,
    message: String,
}

fn llm_provider_config_path_phase18_p3(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("Cannot resolve app config directory: {error}"))?;
    fs::create_dir_all(&config_dir)
        .map_err(|error| format!("Cannot create app config directory: {error}"))?;
    Ok(config_dir.join("llm-provider.json"))
}

fn load_llm_provider_config_phase18_p3(
    app: &AppHandle,
) -> Result<omnicreator_core::LlmProviderConfigV1, String> {
    let path = llm_provider_config_path_phase18_p3(app)?;
    if path.exists() {
        return omnicreator_core::LlmProviderConfigV1::load(path).map_err(error_string);
    }

    let legacy = load_llmgateway_config(app)?;
    omnicreator_core::LlmProviderConfigV1::llmgateway_v1(legacy).map_err(error_string)
}

fn load_llm_provider_phase18_p3(
    app: &AppHandle,
) -> Result<omnicreator_core::ConfiguredLlmProviderV1, String> {
    omnicreator_core::ConfiguredLlmProviderV1::new(load_llm_provider_config_phase18_p3(app)?)
        .map_err(error_string)
}

fn apply_optional_setting_phase18_p3(target: &mut String, value: Option<String>) {
    if let Some(value) = value {
        let value = value.trim();
        if !value.is_empty() {
            *target = value.to_owned();
        }
    }
}

fn save_llm_provider_config_phase18_p3(
    app: &AppHandle,
    provider_kind: &str,
    base_url: Option<String>,
    api_key_env: Option<String>,
    default_model: Option<String>,
) -> Result<(), String> {
    let provider_kind = provider_kind.trim().to_ascii_lowercase();
    let config = match provider_kind.as_str() {
        "llmgateway" | "llm_gateway" => {
            let mut legacy = load_llmgateway_config(app)?;
            apply_optional_setting_phase18_p3(&mut legacy.base_url, base_url);
            apply_optional_setting_phase18_p3(&mut legacy.api_key_env, api_key_env);
            apply_optional_setting_phase18_p3(&mut legacy.default_model, default_model);
            omnicreator_core::LlmProviderConfigV1::llmgateway_v1(legacy).map_err(error_string)?
        }
        "openrouter" | "open_router" => {
            let mut provider = omnicreator_core::OpenAiCompatibleConfigV1::openrouter_v1();
            apply_optional_setting_phase18_p3(&mut provider.base_url, base_url);
            apply_optional_setting_phase18_p3(&mut provider.api_key_env, api_key_env);
            apply_optional_setting_phase18_p3(&mut provider.default_model, default_model);
            provider.validate_v1().map_err(error_string)?;
            omnicreator_core::LlmProviderConfigV1 {
                schema: omnicreator_core::LLM_PROVIDER_CONFIG_SCHEMA_V1.to_owned(),
                version: omnicreator_core::LLM_PROVIDER_CONFIG_VERSION_V1,
                provider: omnicreator_core::LlmProviderProfileV1::OpenRouter { config: provider },
            }
        }
        "open_ai_compatible" | "openai_compatible" => {
            let base_url = base_url
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "OpenAI-compatible provider requires base_url.".to_owned())?;
            let api_key_env = api_key_env
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "OpenAI-compatible provider requires api_key_env.".to_owned())?;
            let default_model = default_model
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "OpenAI-compatible provider requires default_model.".to_owned())?;
            let provider = omnicreator_core::OpenAiCompatibleConfigV1 {
                schema: omnicreator_core::OPENAI_COMPATIBLE_CONFIG_SCHEMA_V1.to_owned(),
                version: omnicreator_core::OPENAI_COMPATIBLE_CONFIG_VERSION_V1,
                provider_id: "openai-compatible".to_owned(),
                provider_kind: omnicreator_core::LlmProviderKindV1::OpenAiCompatible,
                base_url,
                api_key_env,
                default_model,
                timeout_seconds: 60,
                http_referer: None,
                app_title: Some("OmniCreator".to_owned()),
            };
            provider.validate_v1().map_err(error_string)?;
            omnicreator_core::LlmProviderConfigV1 {
                schema: omnicreator_core::LLM_PROVIDER_CONFIG_SCHEMA_V1.to_owned(),
                version: omnicreator_core::LLM_PROVIDER_CONFIG_VERSION_V1,
                provider: omnicreator_core::LlmProviderProfileV1::OpenAiCompatible {
                    config: provider,
                },
            }
        }
        _ => {
            return Err(
                "LLM provider kind must be llmgateway, openrouter, or open_ai_compatible."
                    .to_owned(),
            )
        }
    };
    config.validate_v1().map_err(error_string)?;
    config
        .save(llm_provider_config_path_phase18_p3(app)?)
        .map_err(error_string)
}

fn llm_provider_status_for_app_phase18_p3(
    app: &AppHandle,
) -> Result<LlmProviderDesktopStatusV1, String> {
    let provider = load_llm_provider_phase18_p3(app)?;
    let readiness = omnicreator_core::LlmProviderV1::readiness_v1(&provider);
    let mut state = readiness.state.clone();
    let mut reason_code = readiness.reason_code.clone();
    let mut message = if readiness.credential_present {
        format!(
            "{} is configured for creator intelligence.",
            readiness.provider_id
        )
    } else {
        format!(
            "{} needs credential environment variable {}.",
            readiness.provider_id, readiness.api_key_env
        )
    };
    let models = if readiness.credential_present {
        match omnicreator_core::LlmProviderV1::models_v1(&provider) {
            Ok(models) => {
                state = "ready".to_owned();
                reason_code = None;
                models
            }
            Err(error) => {
                state = "degraded".to_owned();
                reason_code = Some("model_discovery_failed".to_owned());
                message = format!(
                    "{} is configured, but model discovery is currently unavailable: {}",
                    readiness.provider_id, error
                );
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    Ok(LlmProviderDesktopStatusV1 {
        provider_id: readiness.provider_id,
        provider_kind: readiness.kind,
        state,
        base_url: readiness.base_url,
        api_key_env: readiness.api_key_env,
        default_model: readiness.default_model,
        credential_present: readiness.credential_present,
        model_discovery_supported: readiness.model_discovery_supported,
        reason_code,
        models,
        message,
    })
}

#[tauri::command]
fn llm_provider_status_phase18_p3(
    app: AppHandle,
) -> Result<LlmProviderDesktopStatusV1, String> {
    llm_provider_status_for_app_phase18_p3(&app)
}

#[tauri::command]
fn save_llm_provider_settings_phase18_p3(
    app: AppHandle,
    provider_kind: String,
    base_url: Option<String>,
    api_key_env: Option<String>,
    default_model: Option<String>,
) -> Result<LlmProviderDesktopStatusV1, String> {
    save_llm_provider_config_phase18_p3(
        &app,
        &provider_kind,
        base_url,
        api_key_env,
        default_model,
    )?;
    llm_provider_status_for_app_phase18_p3(&app)
}