use std::{env, fs, path::Path, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{fs_util::atomic_write_json, Error, Result};

pub const LLMGATEWAY_CONFIG_SCHEMA: &str = "omnicreator.llmgateway-config";
pub const LLMGATEWAY_CONFIG_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_LLMGATEWAY_URL: &str = "http://127.0.0.1:7331";
pub const DEFAULT_LLMGATEWAY_MODEL: &str = "llmgateway-auto";
pub const DEFAULT_LLMGATEWAY_API_KEY_ENV: &str = "LLMGATEWAY_API_KEY";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmGatewayConfig {
    pub schema: String,
    pub schema_version: u32,
    pub base_url: String,
    pub api_key_env: String,
    pub default_model: String,
    pub timeout_seconds: u64,
}

impl Default for LlmGatewayConfig {
    fn default() -> Self {
        Self {
            schema: LLMGATEWAY_CONFIG_SCHEMA.to_owned(),
            schema_version: LLMGATEWAY_CONFIG_SCHEMA_VERSION,
            base_url: DEFAULT_LLMGATEWAY_URL.to_owned(),
            api_key_env: DEFAULT_LLMGATEWAY_API_KEY_ENV.to_owned(),
            default_model: DEFAULT_LLMGATEWAY_MODEL.to_owned(),
            timeout_seconds: 60,
        }
    }
}

impl LlmGatewayConfig {
    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != LLMGATEWAY_CONFIG_SCHEMA {
            return Err(Error::InvalidLlmGatewayConfig(format!(
                "expected schema {LLMGATEWAY_CONFIG_SCHEMA}, found {}",
                self.schema
            )));
        }
        if self.schema_version != LLMGATEWAY_CONFIG_SCHEMA_VERSION {
            return Err(Error::InvalidLlmGatewayConfig(format!(
                "unsupported schema_version {}",
                self.schema_version
            )));
        }
        let base_url = self.base_url.trim();
        if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
            return Err(Error::InvalidLlmGatewayConfig(
                "base_url must use http:// or https://".to_owned(),
            ));
        }
        if base_url.trim_end_matches('/').is_empty() {
            return Err(Error::InvalidLlmGatewayConfig(
                "base_url must not be empty".to_owned(),
            ));
        }
        if self.api_key_env.trim().is_empty() {
            return Err(Error::InvalidLlmGatewayConfig(
                "api_key_env must not be empty".to_owned(),
            ));
        }
        if self.default_model.trim().is_empty() {
            return Err(Error::InvalidLlmGatewayConfig(
                "default_model must not be empty".to_owned(),
            ));
        }
        if !(1..=600).contains(&self.timeout_seconds) {
            return Err(Error::InvalidLlmGatewayConfig(
                "timeout_seconds must be between 1 and 600".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = fs::read(path)?;
        let config: Self = serde_json::from_slice(&bytes)?;
        config.validate_v1()?;
        Ok(config)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        self.validate_v1()?;
        atomic_write_json(path.as_ref(), self)
    }

    fn endpoint(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim().trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    fn credential(&self) -> Result<String> {
        let value = env::var(&self.api_key_env)
            .map_err(|_| Error::MissingLlmGatewayCredential(self.api_key_env.clone()))?;
        if value.trim().is_empty() {
            return Err(Error::MissingLlmGatewayCredential(self.api_key_env.clone()));
        }
        Ok(value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LlmGatewayTask {
    #[default]
    Auto,
    Coding,
    Reasoning,
    LongContext,
    SimpleChat,
    General,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LlmRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmMessage {
    pub role: LlmRole,
    pub content: String,
}

impl LlmMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: LlmRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: LlmRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: LlmRole::Assistant,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmChatRequest {
    pub model: String,
    pub messages: Vec<LlmMessage>,
    pub stream: bool,
    pub llmgateway_task: LlmGatewayTask,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

impl LlmChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<LlmMessage>) -> Self {
        Self {
            model: model.into(),
            messages,
            stream: false,
            llmgateway_task: LlmGatewayTask::Auto,
            temperature: None,
            max_tokens: None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.model.trim().is_empty() {
            return Err(Error::InvalidLlmGatewayConfig(
                "chat model must not be empty".to_owned(),
            ));
        }
        if self.stream {
            return Err(Error::InvalidLlmGatewayConfig(
                "streaming is not supported by this adapter yet".to_owned(),
            ));
        }
        if self.messages.is_empty()
            || self
                .messages
                .iter()
                .any(|message| message.content.trim().is_empty())
        {
            return Err(Error::InvalidLlmGatewayConfig(
                "chat messages must contain non-empty content".to_owned(),
            ));
        }
        if self.temperature.is_some_and(|value| !value.is_finite()) {
            return Err(Error::InvalidLlmGatewayConfig(
                "temperature must be finite".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmGatewayHealth {
    pub status: String,
    pub service: Option<String>,
    pub default_model: Option<String>,
    pub catalog_models: Option<u64>,
    pub threads: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmGatewayModel {
    pub id: String,
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub owned_by: Option<String>,
    #[serde(default)]
    pub llmgateway: Option<LlmGatewayModelMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmGatewayModelMetadata {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

impl LlmGatewayModel {
    pub fn is_virtual(&self) -> bool {
        self.llmgateway
            .as_ref()
            .and_then(|metadata| metadata.kind.as_deref())
            == Some("virtual")
            || self.id.starts_with("llmgateway-")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmChatResult {
    pub id: Option<String>,
    pub model: Option<String>,
    pub content: String,
}

pub struct LlmGatewayClient {
    config: LlmGatewayConfig,
    agent: ureq::Agent,
}

impl LlmGatewayClient {
    pub fn new(config: LlmGatewayConfig) -> Result<Self> {
        config.validate_v1()?;
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build();
        Ok(Self { config, agent })
    }

    pub fn config(&self) -> &LlmGatewayConfig {
        &self.config
    }

    pub fn health(&self) -> Result<LlmGatewayHealth> {
        let value = self.get_json("/_llmgateway/health", false)?;
        parse_health(value)
    }

    pub fn models(&self) -> Result<Vec<LlmGatewayModel>> {
        let value = self.get_json("/v1/models", true)?;
        parse_models(value)
    }

    pub fn chat(&self, request: &LlmChatRequest) -> Result<LlmChatResult> {
        request.validate()?;
        let value = serde_json::to_value(request)?;
        let response = self.post_json("/v1/chat/completions", &value)?;
        parse_chat_result(response)
    }

    pub fn chat_with_default_model(
        &self,
        messages: Vec<LlmMessage>,
        task: LlmGatewayTask,
    ) -> Result<LlmChatResult> {
        let mut request = LlmChatRequest::new(self.config.default_model.clone(), messages);
        request.llmgateway_task = task;
        self.chat(&request)
    }

    fn get_json(&self, path: &str, authenticated: bool) -> Result<Value> {
        let url = self.config.endpoint(path);
        let mut request = self.agent.get(&url);
        if authenticated {
            let credential = self.config.credential()?;
            let authorization = format!("Bearer {credential}");
            request = request.set("Authorization", &authorization);
        }
        decode_http_response(request.call())
    }

    fn post_json(&self, path: &str, body: &Value) -> Result<Value> {
        let credential = self.config.credential()?;
        let authorization = format!("Bearer {credential}");
        let payload = serde_json::to_string(body)?;
        let url = self.config.endpoint(path);
        let request = self
            .agent
            .post(&url)
            .set("Authorization", &authorization)
            .set("Content-Type", "application/json");
        decode_http_response(request.send_string(&payload))
    }
}

fn decode_http_response(
    response: std::result::Result<ureq::Response, ureq::Error>,
) -> Result<Value> {
    match response {
        Ok(response) => {
            let status = response.status();
            let body = response.into_string()?;
            serde_json::from_str(&body).map_err(|error| {
                Error::InvalidLlmGatewayResponse(format!(
                    "HTTP {status} returned invalid JSON: {error}"
                ))
            })
        }
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            Err(parse_gateway_error(status, &body))
        }
        Err(ureq::Error::Transport(error)) => Err(Error::LlmGatewayTransport(error.to_string())),
    }
}

fn parse_gateway_error(status: u16, body: &str) -> Error {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let error = parsed.as_ref().and_then(|value| value.get("error"));
    let message = error
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("request failed")
        .to_owned();
    let code = error
        .and_then(|value| value.get("code").or_else(|| value.get("type")))
        .and_then(value_as_text);

    Error::LlmGatewayApi {
        status,
        code,
        message,
    }
}

fn value_as_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn parse_health(value: Value) -> Result<LlmGatewayHealth> {
    let health: LlmGatewayHealth = serde_json::from_value(value).map_err(|error| {
        Error::InvalidLlmGatewayResponse(format!("invalid health response: {error}"))
    })?;
    if health.status.trim().is_empty() {
        return Err(Error::InvalidLlmGatewayResponse(
            "health response status is empty".to_owned(),
        ));
    }
    Ok(health)
}

fn parse_models(value: Value) -> Result<Vec<LlmGatewayModel>> {
    #[derive(Deserialize)]
    struct ModelList {
        data: Vec<LlmGatewayModel>,
    }

    let mut models = serde_json::from_value::<ModelList>(value)
        .map_err(|error| {
            Error::InvalidLlmGatewayResponse(format!("invalid model list response: {error}"))
        })?
        .data;

    if models.iter().any(|model| model.id.trim().is_empty()) {
        return Err(Error::InvalidLlmGatewayResponse(
            "model list contains an empty model id".to_owned(),
        ));
    }

    models.sort_by(|left, right| {
        right
            .is_virtual()
            .cmp(&left.is_virtual())
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(models)
}

fn parse_chat_result(value: Value) -> Result<LlmChatResult> {
    #[derive(Deserialize)]
    struct ChatCompletion {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        model: Option<String>,
        choices: Vec<Choice>,
    }

    #[derive(Deserialize)]
    struct Choice {
        message: ChoiceMessage,
    }

    #[derive(Deserialize)]
    struct ChoiceMessage {
        content: Option<String>,
    }

    let response: ChatCompletion = serde_json::from_value(value).map_err(|error| {
        Error::InvalidLlmGatewayResponse(format!("invalid chat response: {error}"))
    })?;
    let content = response
        .choices
        .into_iter()
        .find_map(|choice| choice.message.content)
        .filter(|content| !content.trim().is_empty())
        .ok_or_else(|| {
            Error::InvalidLlmGatewayResponse(
                "chat response does not contain assistant text".to_owned(),
            )
        })?;

    Ok(LlmChatResult {
        id: response.id,
        model: response.model,
        content,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_local_and_does_not_persist_a_secret() {
        let config = LlmGatewayConfig::default();
        config.validate_v1().unwrap();
        let json = serde_json::to_value(&config).unwrap();

        assert_eq!(json["base_url"], DEFAULT_LLMGATEWAY_URL);
        assert_eq!(json["default_model"], DEFAULT_LLMGATEWAY_MODEL);
        assert_eq!(json["api_key_env"], DEFAULT_LLMGATEWAY_API_KEY_ENV);
        assert!(json.get("api_key").is_none());
    }

    #[test]
    fn machine_local_config_round_trips() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("llmgateway.json");
        let config = LlmGatewayConfig::default();

        config.save(&path).unwrap();
        let loaded = LlmGatewayConfig::load(&path).unwrap();

        assert_eq!(loaded, config);
    }

    #[test]
    fn chat_request_carries_explicit_task_hint_without_provider_fields() {
        let mut request = LlmChatRequest::new(
            "llmgateway-auto",
            vec![LlmMessage::user("Evaluate this scene.")],
        );
        request.llmgateway_task = LlmGatewayTask::Reasoning;
        request.max_tokens = Some(800);
        request.validate().unwrap();

        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["model"], "llmgateway-auto");
        assert_eq!(json["llmgateway_task"], "reasoning");
        assert_eq!(json["stream"], false);
        assert!(json.get("provider").is_none());
        assert!(json.get("account").is_none());
        assert!(json.get("route").is_none());
    }

    #[test]
    fn model_parser_places_virtual_models_first() {
        let models = parse_models(serde_json::json!({
            "object":"list",
            "data":[
                {
                    "id":"gemini/gemini-3.7-flash",
                    "object":"model",
                    "owned_by":"google",
                    "llmgateway":{"kind":"physical"}
                },
                {
                    "id":"llmgateway-auto",
                    "object":"model",
                    "owned_by":"llmgateway",
                    "llmgateway":{"kind":"virtual"}
                }
            ]
        }))
        .unwrap();

        assert_eq!(models[0].id, "llmgateway-auto");
        assert!(models[0].is_virtual());
    }

    #[test]
    fn chat_parser_extracts_first_assistant_text() {
        let result = parse_chat_result(serde_json::json!({
            "id":"chatcmpl_123",
            "model":"llmgateway-auto",
            "choices":[{"message":{"role":"assistant","content":"{\"ok\":true}"}}]
        }))
        .unwrap();

        assert_eq!(result.id.as_deref(), Some("chatcmpl_123"));
        assert_eq!(result.content, "{\"ok\":true}");
    }

    #[test]
    fn gateway_error_preserves_type_without_echoing_response_body() {
        let error = parse_gateway_error(
            503,
            r#"{"error":{"type":"browser_session_error","message":"no browser route is ready"},"debug":"do-not-expose"}"#,
        );

        match error {
            Error::LlmGatewayApi {
                status,
                code,
                message,
            } => {
                assert_eq!(status, 503);
                assert_eq!(code.as_deref(), Some("browser_session_error"));
                assert_eq!(message, "no browser route is ready");
                assert!(!message.contains("do-not-expose"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
