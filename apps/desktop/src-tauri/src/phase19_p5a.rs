use std::time::Duration;

const OMNIVOICE_STUDIO_CONFIG_FILE_V1: &str = "omnivoice-studio.json";
const DEFAULT_OMNIVOICE_STUDIO_BASE_URL_V1: &str = "http://127.0.0.1:7860";
const DEFAULT_OMNIVOICE_STUDIO_TOKEN_ENV_V1: &str = "OMNIVOICE_API_TOKEN";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct OmniVoiceStudioRuntimeConfigV1 {
    base_url: String,
    #[serde(default)]
    bearer_token_env: Option<String>,
}

impl Default for OmniVoiceStudioRuntimeConfigV1 {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_OMNIVOICE_STUDIO_BASE_URL_V1.to_owned(),
            bearer_token_env: Some(DEFAULT_OMNIVOICE_STUDIO_TOKEN_ENV_V1.to_owned()),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct OmniVoiceStudioEndpointsV1 {
    base_url: String,
    ui_url: String,
    api_url: String,
    mcp_url: String,
    health_url: String,
    capabilities_url: String,
    generate_audio_url: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum OmniVoiceStudioConnectionStateV1 {
    Ready,
    NeedsApiToken,
    Offline,
    Degraded,
}

#[derive(Debug, Clone, Serialize)]
struct OmniVoiceStudioRuntimeStatusV1 {
    state: OmniVoiceStudioConnectionStateV1,
    base_url: String,
    ui_url: String,
    api_url: String,
    mcp_url: String,
    health_url: String,
    capabilities_url: String,
    generate_audio_url: String,
    bearer_token_env: Option<String>,
    credential_present: bool,
    health_status: Option<String>,
    standalone_audio_generation: Option<bool>,
    mcp_enabled: Option<bool>,
    message: String,
}

fn omnivoice_studio_config_path_v1(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("Cannot resolve app config directory: {error}"))?;
    fs::create_dir_all(&config_dir)
        .map_err(|error| format!("Cannot create app config directory: {error}"))?;
    Ok(config_dir.join(OMNIVOICE_STUDIO_CONFIG_FILE_V1))
}

fn normalize_omnivoice_studio_base_url_v1(raw: &str) -> Result<String, String> {
    let mut candidate = raw.trim().trim_end_matches('/').to_owned();
    if candidate.is_empty() {
        return Err("OmniVoiceStudio URL must not be empty.".to_owned());
    }
    if candidate.chars().any(char::is_whitespace) {
        return Err("OmniVoiceStudio URL must not contain whitespace.".to_owned());
    }
    if candidate.contains('?') || candidate.contains('#') {
        return Err("OmniVoiceStudio URL must not contain query parameters or fragments.".to_owned());
    }

    let lower = candidate.to_ascii_lowercase();
    let scheme_len = if lower.starts_with("https://") {
        "https://".len()
    } else if lower.starts_with("http://") {
        "http://".len()
    } else {
        return Err("OmniVoiceStudio URL must start with http:// or https://.".to_owned());
    };

    for suffix in [
        "/api/v1/capabilities",
        "/api/v1",
        "/mcp",
        "/ui",
        "/health",
    ] {
        if candidate.to_ascii_lowercase().ends_with(suffix) {
            candidate.truncate(candidate.len() - suffix.len());
            candidate = candidate.trim_end_matches('/').to_owned();
            break;
        }
    }

    let authority = candidate
        .get(scheme_len..)
        .ok_or_else(|| "OmniVoiceStudio URL has an invalid scheme.".to_owned())?;
    if authority.is_empty() || authority.starts_with('/') {
        return Err("OmniVoiceStudio URL must include a host.".to_owned());
    }
    if authority.contains('@') {
        return Err(
            "Do not embed credentials in the OmniVoiceStudio URL; use the bearer-token environment variable instead."
                .to_owned(),
        );
    }
    if authority.contains('/') {
        return Err(
            "Use the OmniVoiceStudio root URL, /ui, /api/v1, /mcp, /health, or /api/v1/capabilities URL."
                .to_owned(),
        );
    }

    Ok(candidate)
}

fn omnivoice_studio_endpoints_v1(base_url: &str) -> Result<OmniVoiceStudioEndpointsV1, String> {
    let base_url = normalize_omnivoice_studio_base_url_v1(base_url)?;
    Ok(OmniVoiceStudioEndpointsV1 {
        ui_url: format!("{base_url}/ui"),
        api_url: format!("{base_url}/api/v1"),
        mcp_url: format!("{base_url}/mcp"),
        health_url: format!("{base_url}/health"),
        capabilities_url: format!("{base_url}/api/v1/capabilities"),
        generate_audio_url: format!("{base_url}/api/v1/audio/generate"),
        base_url,
    })
}

fn load_omnivoice_studio_config_v1(
    app: &AppHandle,
) -> Result<OmniVoiceStudioRuntimeConfigV1, String> {
    let path = omnivoice_studio_config_path_v1(app)?;
    let mut config = if path.exists() {
        let raw = fs::read_to_string(&path)
            .map_err(|error| format!("Cannot read OmniVoiceStudio runtime config: {error}"))?;
        serde_json::from_str::<OmniVoiceStudioRuntimeConfigV1>(&raw)
            .map_err(|error| format!("Cannot parse OmniVoiceStudio runtime config: {error}"))?
    } else {
        OmniVoiceStudioRuntimeConfigV1::default()
    };
    config.base_url = normalize_omnivoice_studio_base_url_v1(&config.base_url)?;
    config.bearer_token_env = config
        .bearer_token_env
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    Ok(config)
}

fn save_omnivoice_studio_config_v1(
    app: &AppHandle,
    config: &OmniVoiceStudioRuntimeConfigV1,
) -> Result<(), String> {
    let path = omnivoice_studio_config_path_v1(app)?;
    let json = serde_json::to_string_pretty(config)
        .map_err(|error| format!("Cannot serialize OmniVoiceStudio runtime config: {error}"))?;
    fs::write(path, format!("{json}\n"))
        .map_err(|error| format!("Cannot save OmniVoiceStudio runtime config: {error}"))
}

fn omnivoice_json_get_v1(
    agent: &ureq::Agent,
    url: &str,
    bearer_token: Option<&str>,
) -> Result<serde_json::Value, (Option<u16>, String)> {
    let mut request = agent.get(url).set("Accept", "application/json");
    let authorization = bearer_token.map(|token| format!("Bearer {token}"));
    if let Some(value) = authorization.as_deref() {
        request = request.set("Authorization", value);
    }
    match request.call() {
        Ok(response) => response
            .into_json::<serde_json::Value>()
            .map_err(|error| (None, format!("invalid JSON response: {error}"))),
        Err(ureq::Error::Status(status, _)) => Err((
            Some(status),
            format!("HTTP {status} from OmniVoiceStudio"),
        )),
        Err(ureq::Error::Transport(error)) => Err((None, error.to_string())),
    }
}

fn omnivoice_studio_status_for_app_v1(
    app: &AppHandle,
) -> Result<OmniVoiceStudioRuntimeStatusV1, String> {
    let config = load_omnivoice_studio_config_v1(app)?;
    let endpoints = omnivoice_studio_endpoints_v1(&config.base_url)?;
    let bearer_token = config
        .bearer_token_env
        .as_deref()
        .and_then(|name| env::var(name).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let credential_present = bearer_token.is_some();
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout_read(Duration::from_secs(6))
        .timeout_write(Duration::from_secs(6))
        .build();

    let health = match omnivoice_json_get_v1(&agent, &endpoints.health_url, None) {
        Ok(value) => value,
        Err((_, message)) => {
            return Ok(OmniVoiceStudioRuntimeStatusV1 {
                state: OmniVoiceStudioConnectionStateV1::Offline,
                base_url: endpoints.base_url,
                ui_url: endpoints.ui_url,
                api_url: endpoints.api_url,
                mcp_url: endpoints.mcp_url,
                health_url: endpoints.health_url,
                capabilities_url: endpoints.capabilities_url,
                generate_audio_url: endpoints.generate_audio_url,
                bearer_token_env: config.bearer_token_env,
                credential_present,
                health_status: None,
                standalone_audio_generation: None,
                mcp_enabled: None,
                message: format!(
                    "OmniVoiceStudio is not reachable at the configured runtime domain. Update the domain and refresh. ({message})"
                ),
            });
        }
    };
    let health_status = health
        .get("status")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);

    let capabilities = match omnivoice_json_get_v1(
        &agent,
        &endpoints.capabilities_url,
        bearer_token.as_deref(),
    ) {
        Ok(value) => value,
        Err((Some(401), _)) if !credential_present => {
            let env_name = config
                .bearer_token_env
                .clone()
                .unwrap_or_else(|| DEFAULT_OMNIVOICE_STUDIO_TOKEN_ENV_V1.to_owned());
            return Ok(OmniVoiceStudioRuntimeStatusV1 {
                state: OmniVoiceStudioConnectionStateV1::NeedsApiToken,
                base_url: endpoints.base_url,
                ui_url: endpoints.ui_url,
                api_url: endpoints.api_url,
                mcp_url: endpoints.mcp_url,
                health_url: endpoints.health_url,
                capabilities_url: endpoints.capabilities_url,
                generate_audio_url: endpoints.generate_audio_url,
                bearer_token_env: config.bearer_token_env,
                credential_present,
                health_status,
                standalone_audio_generation: None,
                mcp_enabled: None,
                message: format!(
                    "OmniVoiceStudio is reachable but its machine API requires a bearer token. Set {env_name} on this machine and refresh. The token value is never stored in OmniCreator."
                ),
            });
        }
        Err((Some(status), message)) => {
            return Ok(OmniVoiceStudioRuntimeStatusV1 {
                state: OmniVoiceStudioConnectionStateV1::Degraded,
                base_url: endpoints.base_url,
                ui_url: endpoints.ui_url,
                api_url: endpoints.api_url,
                mcp_url: endpoints.mcp_url,
                health_url: endpoints.health_url,
                capabilities_url: endpoints.capabilities_url,
                generate_audio_url: endpoints.generate_audio_url,
                bearer_token_env: config.bearer_token_env,
                credential_present,
                health_status,
                standalone_audio_generation: None,
                mcp_enabled: None,
                message: format!(
                    "OmniVoiceStudio health is reachable, but capability discovery returned HTTP {status}. Check the configured token/scope and runtime domain. ({message})"
                ),
            });
        }
        Err((None, message)) => {
            return Ok(OmniVoiceStudioRuntimeStatusV1 {
                state: OmniVoiceStudioConnectionStateV1::Degraded,
                base_url: endpoints.base_url,
                ui_url: endpoints.ui_url,
                api_url: endpoints.api_url,
                mcp_url: endpoints.mcp_url,
                health_url: endpoints.health_url,
                capabilities_url: endpoints.capabilities_url,
                generate_audio_url: endpoints.generate_audio_url,
                bearer_token_env: config.bearer_token_env,
                credential_present,
                health_status,
                standalone_audio_generation: None,
                mcp_enabled: None,
                message: format!(
                    "OmniVoiceStudio health is reachable, but capability discovery failed. ({message})"
                ),
            });
        }
    };

    let standalone_audio_generation = capabilities
        .pointer("/features/standalone_audio_generation")
        .and_then(serde_json::Value::as_bool);
    let mcp_enabled = capabilities
        .pointer("/features/mcp")
        .and_then(serde_json::Value::as_bool);
    let state = if standalone_audio_generation == Some(false) {
        OmniVoiceStudioConnectionStateV1::Degraded
    } else {
        OmniVoiceStudioConnectionStateV1::Ready
    };
    let message = if state == OmniVoiceStudioConnectionStateV1::Ready {
        if credential_present {
            "Connected to OmniVoiceStudio. REST voice generation will use this runtime domain and the machine bearer token environment variable.".to_owned()
        } else {
            "Connected to OmniVoiceStudio. REST voice generation is reachable without a machine bearer credential on this runtime.".to_owned()
        }
    } else {
        "OmniVoiceStudio is reachable, but standalone audio generation is not advertised by this runtime.".to_owned()
    };

    Ok(OmniVoiceStudioRuntimeStatusV1 {
        state,
        base_url: endpoints.base_url,
        ui_url: endpoints.ui_url,
        api_url: endpoints.api_url,
        mcp_url: endpoints.mcp_url,
        health_url: endpoints.health_url,
        capabilities_url: endpoints.capabilities_url,
        generate_audio_url: endpoints.generate_audio_url,
        bearer_token_env: config.bearer_token_env,
        credential_present,
        health_status,
        standalone_audio_generation,
        mcp_enabled,
        message,
    })
}

#[tauri::command]
fn omnivoice_studio_status(
    app: AppHandle,
) -> Result<OmniVoiceStudioRuntimeStatusV1, String> {
    omnivoice_studio_status_for_app_v1(&app)
}

#[tauri::command]
fn save_omnivoice_studio_settings(
    app: AppHandle,
    base_url: String,
    bearer_token_env: Option<String>,
) -> Result<OmniVoiceStudioRuntimeStatusV1, String> {
    let config = OmniVoiceStudioRuntimeConfigV1 {
        base_url: normalize_omnivoice_studio_base_url_v1(&base_url)?,
        bearer_token_env: bearer_token_env
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
    };
    save_omnivoice_studio_config_v1(&app, &config)?;
    omnivoice_studio_status_for_app_v1(&app)
}

pub(super) fn run_phase19_p5a() {
    let app = tauri::Builder::default()
        .manage(DesktopState::default())
        .invoke_handler(tauri::generate_handler![
            pick_data_root,
            pick_plugin_folder,
            bootstrap,
            create_data_root,
            use_existing_data_root,
            open_read_only,
            check_again,
            heartbeat,
            list_projects,
            create_project,
            create_project_from_studio_pack,
            update_project_studio_pack,
            plugin_inventory,
            set_plugin_enabled,
            install_plugin_from_folder,
            uninstall_plugin,
            inspect_plugin_update,
            apply_plugin_update,
            plugin_mutation_impact,
            studio_pack_catalog,
            asset_library,
            add_asset_tag,
            remove_asset_tag,
            review_center,
            retry_review_job,
            creator_visual_review_status,
            creator_manual_visual_status,
            creator_manual_voice_status,
            select_creator_visual_candidate,
            approve_creator_generated_visual,
            use_creator_image_manually,
            use_creator_video_manually,
            choose_creator_asset_library_visual,
            export_creator_external_visual_request,
            provide_creator_external_visual_result,
            use_creator_wav_manually,
            use_creator_audio_with_timing_manually,
            export_creator_external_voice_request,
            provide_creator_external_voice_result,
            replace_creator_voice_timing_manually,
            provide_creator_script_manually,
            import_creator_script_manually,
            creator_manual_scene_editor,
            provide_creator_scene_plan_manually,
            import_creator_scene_plan_manually,
            start_creator_production_phase17,
            workflow_step_execution_policies,
            set_workflow_step_automatic_execution,
            rename_project,
            delete_project,
            production_export_status,
            production_recovery_status,
            repair_production_visual,
            repair_production_audio,
            repair_production_timing,
            repair_production_voice_bundle,
            rebuild_recovered_production,
            assemble_production_pack,
            export_production_pack,
            llmgateway_status,
            save_llmgateway_settings,
            llm_provider_status_phase18_p3,
            save_llm_provider_settings_phase18_p3,
            omnivoice_studio_status,
            save_omnivoice_studio_settings,
            compute_provider_status,
            connect_compute_provider,
            disconnect_compute_provider,
            sync_compute_burst,
            gpu_workbench_review,
            set_gpu_weekly_budget,
            start_gpu_burst,
            prepare_device_handoff
        ])
        .build(tauri::generate_context!())
        .expect("failed to build OmniCreator desktop");

    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            let state = app_handle.state::<DesktopState>();
            clean_shutdown(&state);
        }
    });
}

#[cfg(test)]
mod omnivoice_studio_phase19_p5a_tests {
    use super::*;

    #[test]
    fn runtime_url_normalization_accepts_all_studio_surfaces() {
        let base = "https://neo-station-pavilion-bay.trycloudflare.com";
        for candidate in [
            base.to_owned(),
            format!("{base}/"),
            format!("{base}/ui"),
            format!("{base}/api/v1"),
            format!("{base}/mcp"),
            format!("{base}/health"),
            format!("{base}/api/v1/capabilities"),
        ] {
            assert_eq!(
                normalize_omnivoice_studio_base_url_v1(&candidate).unwrap(),
                base
            );
        }
    }

    #[test]
    fn runtime_url_derives_current_omnivoice_contract() {
        let base = "https://new-runtime.trycloudflare.com";
        let endpoints = omnivoice_studio_endpoints_v1(base).unwrap();
        assert_eq!(endpoints.ui_url, format!("{base}/ui"));
        assert_eq!(endpoints.api_url, format!("{base}/api/v1"));
        assert_eq!(endpoints.mcp_url, format!("{base}/mcp"));
        assert_eq!(endpoints.health_url, format!("{base}/health"));
        assert_eq!(
            endpoints.generate_audio_url,
            format!("{base}/api/v1/audio/generate")
        );
    }

    #[test]
    fn runtime_url_rejects_credentials_queries_and_unknown_paths() {
        for candidate in [
            "ftp://studio.example.com",
            "https://user:secret@studio.example.com",
            "https://studio.example.com/ui?token=secret",
            "https://studio.example.com/custom/path",
        ] {
            assert!(normalize_omnivoice_studio_base_url_v1(candidate).is_err());
        }
    }

    #[test]
    fn machine_config_persists_env_name_not_secret_value() {
        let config = OmniVoiceStudioRuntimeConfigV1 {
            base_url: "https://runtime.trycloudflare.com".to_owned(),
            bearer_token_env: Some("OMNIVOICE_API_TOKEN".to_owned()),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("OMNIVOICE_API_TOKEN"));
        assert!(!json.contains("Bearer "));
        assert!(!json.contains("secret-value"));
    }
}
