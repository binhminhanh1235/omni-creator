use std::{env, fs, path::Path, time::Duration};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    validate_generated_scene_intent, CreatorContentOptions, CreatorInputKindV1, CreatorInputV1,
    CreatorLlmExecutorV1, CreatorQualityOptions, Error, LlmChatRequest, LlmChatResult,
    LlmGatewayClient, LlmGatewayConfig, LlmGatewayModel, LlmGatewayTask, LlmMessage, Result,
    SceneIntentGenerationOptions, SceneIntentV1, SegmentV1, SCENE_INTENT_SCHEMA,
    SCENE_INTENT_SCHEMA_VERSION,
};

pub const LLM_PROVIDER_CONFIG_SCHEMA_V1: &str = "omnicreator.llm-provider-config";
pub const LLM_PROVIDER_CONFIG_VERSION_V1: u32 = 1;
pub const OPENAI_COMPATIBLE_CONFIG_SCHEMA_V1: &str = "omnicreator.openai-compatible-config";
pub const OPENAI_COMPATIBLE_CONFIG_VERSION_V1: u32 = 1;
pub const DEFAULT_OPENROUTER_BASE_URL_V1: &str = "https://openrouter.ai/api/v1";
pub const DEFAULT_OPENROUTER_API_KEY_ENV_V1: &str = "OPENROUTER_API_KEY";
pub const DEFAULT_OPENROUTER_MODEL_V1: &str = "openrouter/auto";
pub const DEFAULT_OPENROUTER_PROVIDER_ID_V1: &str = "openrouter";

const DEFAULT_PROVIDER_TIMEOUT_SECONDS_V1: u64 = 60;
const MIN_SCENE_VISUAL_IDEAS_V1: usize = 2;
const MIN_SCENE_SEARCH_QUERIES_V1: usize = 3;
const MAX_SCENE_SEARCH_QUERIES_V1: usize = 6;
const CREATOR_SCRIPT_INSTRUCTION_V1: &str = "Write a production-ready narration script from the supplied topic. Preserve the creator's core intent, use clear spoken language, and separate meaningful visual beats with blank lines. Return only the narration script, without markdown fences, provider names, routing details, or production metadata.";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmProviderKindV1 {
    LlmGateway,
    OpenAiCompatible,
    OpenRouter,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmTaskV1 {
    Auto,
    Coding,
    Reasoning,
    LongContext,
    SimpleChat,
    General,
}

impl From<LlmGatewayTask> for LlmTaskV1 {
    fn from(value: LlmGatewayTask) -> Self {
        match value {
            LlmGatewayTask::Auto => Self::Auto,
            LlmGatewayTask::Coding => Self::Coding,
            LlmGatewayTask::Reasoning => Self::Reasoning,
            LlmGatewayTask::LongContext => Self::LongContext,
            LlmGatewayTask::SimpleChat => Self::SimpleChat,
            LlmGatewayTask::General => Self::General,
        }
    }
}

impl From<LlmTaskV1> for LlmGatewayTask {
    fn from(value: LlmTaskV1) -> Self {
        match value {
            LlmTaskV1::Auto => Self::Auto,
            LlmTaskV1::Coding => Self::Coding,
            LlmTaskV1::Reasoning => Self::Reasoning,
            LlmTaskV1::LongContext => Self::LongContext,
            LlmTaskV1::SimpleChat => Self::SimpleChat,
            LlmTaskV1::General => Self::General,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LlmProviderChatRequestV1 {
    pub model: Option<String>,
    pub messages: Vec<LlmMessage>,
    pub task: LlmTaskV1,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
}

impl LlmProviderChatRequestV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self
            .model
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(Error::InvalidLlmProviderConfig(
                "LLM model must not be empty when present".to_owned(),
            ));
        }
        if self.messages.is_empty()
            || self
                .messages
                .iter()
                .any(|message| message.content.trim().is_empty())
        {
            return Err(Error::InvalidLlmProviderConfig(
                "LLM messages must contain non-empty content".to_owned(),
            ));
        }
        if self.temperature.is_some_and(|value| !value.is_finite()) {
            return Err(Error::InvalidLlmProviderConfig(
                "LLM temperature must be finite".to_owned(),
            ));
        }
        if self.max_tokens == Some(0) {
            return Err(Error::InvalidLlmProviderConfig(
                "LLM max_tokens must be positive when present".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LlmProviderModelV1 {
    pub id: String,
    pub display_name: String,
    pub provider_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LlmProviderReadinessV1 {
    pub provider_id: String,
    pub kind: LlmProviderKindV1,
    pub state: String,
    pub base_url: String,
    pub api_key_env: String,
    pub default_model: String,
    pub credential_present: bool,
    pub model_discovery_supported: bool,
    pub reason_code: Option<String>,
}

pub trait LlmProviderV1 {
    fn provider_id_v1(&self) -> &str;
    fn kind_v1(&self) -> LlmProviderKindV1;
    fn base_url_v1(&self) -> &str;
    fn api_key_env_v1(&self) -> &str;
    fn default_model_v1(&self) -> &str;
    fn chat_v1(&self, request: &LlmProviderChatRequestV1) -> Result<LlmChatResult>;
    fn models_v1(&self) -> Result<Vec<LlmProviderModelV1>>;

    fn readiness_v1(&self) -> LlmProviderReadinessV1 {
        let credential_present =
            env::var(self.api_key_env_v1()).is_ok_and(|value| !value.trim().is_empty());
        LlmProviderReadinessV1 {
            provider_id: self.provider_id_v1().to_owned(),
            kind: self.kind_v1(),
            state: if credential_present {
                "configured".to_owned()
            } else {
                "setup_required".to_owned()
            },
            base_url: self.base_url_v1().to_owned(),
            api_key_env: self.api_key_env_v1().to_owned(),
            default_model: self.default_model_v1().to_owned(),
            credential_present,
            model_discovery_supported: true,
            reason_code: (!credential_present).then(|| "credential_env_missing".to_owned()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OpenAiCompatibleConfigV1 {
    pub schema: String,
    pub version: u32,
    pub provider_id: String,
    pub provider_kind: LlmProviderKindV1,
    pub base_url: String,
    pub api_key_env: String,
    pub default_model: String,
    pub timeout_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_referer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_title: Option<String>,
}

impl OpenAiCompatibleConfigV1 {
    pub fn openrouter_v1() -> Self {
        Self {
            schema: OPENAI_COMPATIBLE_CONFIG_SCHEMA_V1.to_owned(),
            version: OPENAI_COMPATIBLE_CONFIG_VERSION_V1,
            provider_id: DEFAULT_OPENROUTER_PROVIDER_ID_V1.to_owned(),
            provider_kind: LlmProviderKindV1::OpenRouter,
            base_url: DEFAULT_OPENROUTER_BASE_URL_V1.to_owned(),
            api_key_env: DEFAULT_OPENROUTER_API_KEY_ENV_V1.to_owned(),
            default_model: DEFAULT_OPENROUTER_MODEL_V1.to_owned(),
            timeout_seconds: DEFAULT_PROVIDER_TIMEOUT_SECONDS_V1,
            http_referer: None,
            app_title: Some("OmniCreator".to_owned()),
        }
    }

    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != OPENAI_COMPATIBLE_CONFIG_SCHEMA_V1
            || self.version != OPENAI_COMPATIBLE_CONFIG_VERSION_V1
        {
            return Err(Error::InvalidLlmProviderConfig(
                "unsupported OpenAI-compatible config schema/version".to_owned(),
            ));
        }
        if !matches!(
            self.provider_kind,
            LlmProviderKindV1::OpenAiCompatible | LlmProviderKindV1::OpenRouter
        ) {
            return Err(Error::InvalidLlmProviderConfig(
                "OpenAI-compatible config must use open_ai_compatible or open_router provider kind"
                    .to_owned(),
            ));
        }
        if self.provider_id.trim().is_empty() {
            return Err(Error::InvalidLlmProviderConfig(
                "provider_id must not be empty".to_owned(),
            ));
        }
        let base_url = self.base_url.trim().trim_end_matches('/');
        if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
            return Err(Error::InvalidLlmProviderConfig(
                "base_url must use http or https".to_owned(),
            ));
        }
        if self.api_key_env.trim().is_empty()
            || !self
                .api_key_env
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
        {
            return Err(Error::InvalidLlmProviderConfig(
                "api_key_env must be a non-empty uppercase environment variable name".to_owned(),
            ));
        }
        if self.default_model.trim().is_empty() {
            return Err(Error::InvalidLlmProviderConfig(
                "default_model must not be empty".to_owned(),
            ));
        }
        if !(1..=600).contains(&self.timeout_seconds) {
            return Err(Error::InvalidLlmProviderConfig(
                "timeout_seconds must be between 1 and 600".to_owned(),
            ));
        }
        for (label, value) in [
            ("http_referer", self.http_referer.as_deref()),
            ("app_title", self.app_title.as_deref()),
        ] {
            if value.is_some_and(|value| value.trim().is_empty()) {
                return Err(Error::InvalidLlmProviderConfig(format!(
                    "{label} must not be empty when present"
                )));
            }
        }
        Ok(())
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let config: Self = serde_json::from_slice(&fs::read(path)?)?;
        config.validate_v1()?;
        Ok(config)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        self.validate_v1()?;
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    fn endpoint_v1(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim().trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    fn credential_v1(&self) -> Result<String> {
        env::var(&self.api_key_env)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::MissingLlmProviderCredential {
                provider: self.provider_id.clone(),
                env: self.api_key_env.clone(),
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LlmProviderProfileV1 {
    LlmGateway { config: LlmGatewayConfig },
    OpenAiCompatible { config: OpenAiCompatibleConfigV1 },
    OpenRouter { config: OpenAiCompatibleConfigV1 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LlmProviderConfigV1 {
    pub schema: String,
    pub version: u32,
    pub provider: LlmProviderProfileV1,
}

impl LlmProviderConfigV1 {
    pub fn llmgateway_v1(config: LlmGatewayConfig) -> Result<Self> {
        config.validate_v1()?;
        Ok(Self {
            schema: LLM_PROVIDER_CONFIG_SCHEMA_V1.to_owned(),
            version: LLM_PROVIDER_CONFIG_VERSION_V1,
            provider: LlmProviderProfileV1::LlmGateway { config },
        })
    }

    pub fn openrouter_v1() -> Self {
        Self {
            schema: LLM_PROVIDER_CONFIG_SCHEMA_V1.to_owned(),
            version: LLM_PROVIDER_CONFIG_VERSION_V1,
            provider: LlmProviderProfileV1::OpenRouter {
                config: OpenAiCompatibleConfigV1::openrouter_v1(),
            },
        }
    }

    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != LLM_PROVIDER_CONFIG_SCHEMA_V1
            || self.version != LLM_PROVIDER_CONFIG_VERSION_V1
        {
            return Err(Error::InvalidLlmProviderConfig(
                "unsupported LLM provider config schema/version".to_owned(),
            ));
        }
        match &self.provider {
            LlmProviderProfileV1::LlmGateway { config } => config.validate_v1(),
            LlmProviderProfileV1::OpenAiCompatible { config } => {
                config.validate_v1()?;
                if config.provider_kind != LlmProviderKindV1::OpenAiCompatible {
                    return Err(Error::InvalidLlmProviderConfig(
                        "open_ai_compatible profile must use open_ai_compatible provider kind"
                            .to_owned(),
                    ));
                }
                Ok(())
            }
            LlmProviderProfileV1::OpenRouter { config } => {
                config.validate_v1()?;
                if config.provider_kind != LlmProviderKindV1::OpenRouter {
                    return Err(Error::InvalidLlmProviderConfig(
                        "open_router profile must use open_router provider kind".to_owned(),
                    ));
                }
                Ok(())
            }
        }
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let config: Self = serde_json::from_slice(&fs::read(path)?)?;
        config.validate_v1()?;
        Ok(config)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        self.validate_v1()?;
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}

pub struct LlmGatewayProviderV1 {
    client: LlmGatewayClient,
}

impl LlmGatewayProviderV1 {
    pub fn new(config: LlmGatewayConfig) -> Result<Self> {
        Ok(Self {
            client: LlmGatewayClient::new(config)?,
        })
    }

    pub fn client(&self) -> &LlmGatewayClient {
        &self.client
    }
}

impl LlmProviderV1 for LlmGatewayProviderV1 {
    fn provider_id_v1(&self) -> &str {
        "llmgateway"
    }

    fn kind_v1(&self) -> LlmProviderKindV1 {
        LlmProviderKindV1::LlmGateway
    }

    fn base_url_v1(&self) -> &str {
        &self.client.config().base_url
    }

    fn api_key_env_v1(&self) -> &str {
        &self.client.config().api_key_env
    }

    fn default_model_v1(&self) -> &str {
        &self.client.config().default_model
    }

    fn chat_v1(&self, request: &LlmProviderChatRequestV1) -> Result<LlmChatResult> {
        request.validate_v1()?;
        let mut gateway_request = LlmChatRequest::new(
            request
                .model
                .clone()
                .unwrap_or_else(|| self.default_model_v1().to_owned()),
            request.messages.clone(),
        );
        gateway_request.llmgateway_task = request.task.into();
        gateway_request.temperature = request.temperature;
        gateway_request.max_tokens = request.max_tokens;
        self.client.chat(&gateway_request)
    }

    fn models_v1(&self) -> Result<Vec<LlmProviderModelV1>> {
        self.client
            .models()?
            .into_iter()
            .map(|model| gateway_model_v1(self.provider_id_v1(), model))
            .collect()
    }
}

pub struct OpenAiCompatibleProviderV1 {
    config: OpenAiCompatibleConfigV1,
    agent: ureq::Agent,
}

impl OpenAiCompatibleProviderV1 {
    pub fn new(config: OpenAiCompatibleConfigV1) -> Result<Self> {
        config.validate_v1()?;
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build();
        Ok(Self { config, agent })
    }

    pub fn config(&self) -> &OpenAiCompatibleConfigV1 {
        &self.config
    }

    fn authenticated_get_v1(&self, path: &str) -> Result<Value> {
        let credential = self.config.credential_v1()?;
        let authorization = format!("Bearer {credential}");
        let request = self
            .agent
            .get(&self.config.endpoint_v1(path))
            .set("Authorization", &authorization);
        decode_provider_http_v1(&self.config.provider_id, request.call())
    }

    fn authenticated_post_v1(&self, path: &str, body: &Value) -> Result<Value> {
        let credential = self.config.credential_v1()?;
        let authorization = format!("Bearer {credential}");
        let payload = serde_json::to_string(body)?;
        let mut request = self
            .agent
            .post(&self.config.endpoint_v1(path))
            .set("Authorization", &authorization)
            .set("Content-Type", "application/json");
        if let Some(referer) = self.config.http_referer.as_deref() {
            request = request.set("HTTP-Referer", referer);
        }
        if let Some(title) = self.config.app_title.as_deref() {
            request = request.set("X-Title", title);
        }
        decode_provider_http_v1(&self.config.provider_id, request.send_string(&payload))
    }
}

impl LlmProviderV1 for OpenAiCompatibleProviderV1 {
    fn provider_id_v1(&self) -> &str {
        &self.config.provider_id
    }

    fn kind_v1(&self) -> LlmProviderKindV1 {
        self.config.provider_kind
    }

    fn base_url_v1(&self) -> &str {
        &self.config.base_url
    }

    fn api_key_env_v1(&self) -> &str {
        &self.config.api_key_env
    }

    fn default_model_v1(&self) -> &str {
        &self.config.default_model
    }

    fn chat_v1(&self, request: &LlmProviderChatRequestV1) -> Result<LlmChatResult> {
        request.validate_v1()?;
        let model = request
            .model
            .as_deref()
            .unwrap_or_else(|| self.default_model_v1());
        let mut body = serde_json::Map::new();
        body.insert("model".to_owned(), Value::String(model.to_owned()));
        body.insert(
            "messages".to_owned(),
            serde_json::to_value(&request.messages)?,
        );
        body.insert("stream".to_owned(), Value::Bool(false));
        if let Some(temperature) = request.temperature {
            body.insert("temperature".to_owned(), json!(temperature));
        }
        if let Some(max_tokens) = request.max_tokens {
            body.insert("max_tokens".to_owned(), json!(max_tokens));
        }
        let value = self.authenticated_post_v1("/chat/completions", &Value::Object(body))?;
        parse_provider_chat_result_v1(&self.config.provider_id, value)
    }

    fn models_v1(&self) -> Result<Vec<LlmProviderModelV1>> {
        let value = self.authenticated_get_v1("/models")?;
        parse_provider_models_v1(&self.config.provider_id, value)
    }
}

pub enum ConfiguredLlmProviderV1 {
    LlmGateway(LlmGatewayProviderV1),
    OpenAiCompatible(OpenAiCompatibleProviderV1),
}

impl ConfiguredLlmProviderV1 {
    pub fn new(config: LlmProviderConfigV1) -> Result<Self> {
        config.validate_v1()?;
        match config.provider {
            LlmProviderProfileV1::LlmGateway { config } => {
                Ok(Self::LlmGateway(LlmGatewayProviderV1::new(config)?))
            }
            LlmProviderProfileV1::OpenAiCompatible { config }
            | LlmProviderProfileV1::OpenRouter { config } => Ok(Self::OpenAiCompatible(
                OpenAiCompatibleProviderV1::new(config)?,
            )),
        }
    }
}

impl LlmProviderV1 for ConfiguredLlmProviderV1 {
    fn provider_id_v1(&self) -> &str {
        match self {
            Self::LlmGateway(provider) => provider.provider_id_v1(),
            Self::OpenAiCompatible(provider) => provider.provider_id_v1(),
        }
    }

    fn kind_v1(&self) -> LlmProviderKindV1 {
        match self {
            Self::LlmGateway(provider) => provider.kind_v1(),
            Self::OpenAiCompatible(provider) => provider.kind_v1(),
        }
    }

    fn base_url_v1(&self) -> &str {
        match self {
            Self::LlmGateway(provider) => provider.base_url_v1(),
            Self::OpenAiCompatible(provider) => provider.base_url_v1(),
        }
    }

    fn api_key_env_v1(&self) -> &str {
        match self {
            Self::LlmGateway(provider) => provider.api_key_env_v1(),
            Self::OpenAiCompatible(provider) => provider.api_key_env_v1(),
        }
    }

    fn default_model_v1(&self) -> &str {
        match self {
            Self::LlmGateway(provider) => provider.default_model_v1(),
            Self::OpenAiCompatible(provider) => provider.default_model_v1(),
        }
    }

    fn chat_v1(&self, request: &LlmProviderChatRequestV1) -> Result<LlmChatResult> {
        match self {
            Self::LlmGateway(provider) => provider.chat_v1(request),
            Self::OpenAiCompatible(provider) => provider.chat_v1(request),
        }
    }

    fn models_v1(&self) -> Result<Vec<LlmProviderModelV1>> {
        match self {
            Self::LlmGateway(provider) => provider.models_v1(),
            Self::OpenAiCompatible(provider) => provider.models_v1(),
        }
    }
}

impl CreatorLlmExecutorV1 for LlmGatewayProviderV1 {
    fn create_script_v1(&self, input: &CreatorInputV1) -> Result<String> {
        create_script_with_provider_v1(self, input)
    }

    fn create_scene_intent_v1(
        &self,
        segment: &SegmentV1,
        scene_id: &str,
        options: &crate::CreatorContentSceneOptionsV1,
    ) -> Result<SceneIntentV1> {
        create_scene_with_provider_v1(self, segment, scene_id, options)
    }
}

impl CreatorLlmExecutorV1 for OpenAiCompatibleProviderV1 {
    fn create_script_v1(&self, input: &CreatorInputV1) -> Result<String> {
        create_script_with_provider_v1(self, input)
    }

    fn create_scene_intent_v1(
        &self,
        segment: &SegmentV1,
        scene_id: &str,
        options: &crate::CreatorContentSceneOptionsV1,
    ) -> Result<SceneIntentV1> {
        create_scene_with_provider_v1(self, segment, scene_id, options)
    }
}

impl CreatorLlmExecutorV1 for ConfiguredLlmProviderV1 {
    fn create_script_v1(&self, input: &CreatorInputV1) -> Result<String> {
        create_script_with_provider_v1(self, input)
    }

    fn create_scene_intent_v1(
        &self,
        segment: &SegmentV1,
        scene_id: &str,
        options: &crate::CreatorContentSceneOptionsV1,
    ) -> Result<SceneIntentV1> {
        create_scene_with_provider_v1(self, segment, scene_id, options)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructuredLlmRequestOptionsV1 {
    pub contract: String,
    pub model: Option<String>,
    pub task: LlmTaskV1,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub max_attempts: u8,
}

impl StructuredLlmRequestOptionsV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.contract.trim().is_empty() {
            return Err(Error::InvalidLlmProviderConfig(
                "structured LLM contract must not be empty".to_owned(),
            ));
        }
        if self
            .model
            .as_ref()
            .is_some_and(|model| model.trim().is_empty())
        {
            return Err(Error::InvalidLlmProviderConfig(
                "structured LLM model must not be empty when present".to_owned(),
            ));
        }
        if !(1..=4).contains(&self.max_attempts) {
            return Err(Error::InvalidLlmProviderConfig(
                "structured LLM max_attempts must be between 1 and 4".to_owned(),
            ));
        }
        if self.temperature.is_some_and(|value| !value.is_finite()) {
            return Err(Error::InvalidLlmProviderConfig(
                "structured LLM temperature must be finite".to_owned(),
            ));
        }
        Ok(())
    }
}

pub fn chat_structured_with_provider_v1<T, F>(
    provider: &dyn LlmProviderV1,
    messages: Vec<LlmMessage>,
    options: &StructuredLlmRequestOptionsV1,
    validate: F,
) -> Result<T>
where
    T: DeserializeOwned,
    F: Fn(&T) -> Result<()>,
{
    options.validate_v1()?;
    if messages.is_empty() {
        return Err(Error::InvalidLlmProviderConfig(
            "structured LLM messages must not be empty".to_owned(),
        ));
    }
    let mut attempt_messages = messages;
    let mut last_reason = "structured output was not attempted".to_owned();
    for attempt in 1..=options.max_attempts {
        let response = provider.chat_v1(&LlmProviderChatRequestV1 {
            model: options.model.clone(),
            messages: attempt_messages.clone(),
            task: options.task,
            temperature: options.temperature,
            max_tokens: options.max_tokens,
        })?;
        match decode_structured_output_v1::<T, F>(&response.content, &validate) {
            Ok(value) => return Ok(value),
            Err(reason) => {
                last_reason = reason;
                if attempt == options.max_attempts {
                    break;
                }
                attempt_messages.push(LlmMessage::assistant(response.content));
                attempt_messages.push(LlmMessage::user(structured_repair_prompt_v1(
                    &options.contract,
                    &last_reason,
                )));
            }
        }
    }
    Err(Error::InvalidStructuredOutput {
        contract: options.contract.clone(),
        attempts: options.max_attempts,
        reason: last_reason,
    })
}

pub fn evaluate_quality_with_provider_v1<T, F>(
    provider: &dyn LlmProviderV1,
    contract: &str,
    rubric: &str,
    candidate: &str,
    options: &CreatorQualityOptions,
    validate: F,
) -> Result<T>
where
    T: DeserializeOwned,
    F: Fn(&T) -> Result<()>,
{
    if contract.trim().is_empty() || rubric.trim().is_empty() || candidate.trim().is_empty() {
        return Err(Error::InvalidContract(
            "quality contract, rubric, and candidate must not be empty".to_owned(),
        ));
    }
    let messages = vec![
        LlmMessage::system("You are OmniCreator Quality Intelligence. Evaluate only the supplied artifact against the rubric. Remain provider-neutral. Never choose or report LLM providers, accounts, URLs, or routing internals. Return only the JSON object required by the requested contract."),
        LlmMessage::user(format!("Quality contract: {contract}\nRubric:\n{rubric}\n\nCandidate:\n{candidate}\n\nReturn only the JSON value for contract '{contract}'.")),
    ];
    let request = StructuredLlmRequestOptionsV1 {
        contract: contract.to_owned(),
        model: options.model.clone(),
        task: LlmTaskV1::Reasoning,
        temperature: Some(0.0),
        max_tokens: options.max_tokens,
        max_attempts: options.max_attempts,
    };
    chat_structured_with_provider_v1(provider, messages, &request, validate)
}

fn create_script_with_provider_v1(
    provider: &dyn LlmProviderV1,
    input: &CreatorInputV1,
) -> Result<String> {
    input.validate_v1()?;
    match input.kind {
        CreatorInputKindV1::Script => Ok(input.text.trim().to_owned()),
        CreatorInputKindV1::Topic => {
            let options = CreatorContentOptions::default();
            let result = provider.chat_v1(&LlmProviderChatRequestV1 {
                model: options.model,
                messages: vec![
                    LlmMessage::system(CREATOR_SCRIPT_INSTRUCTION_V1),
                    LlmMessage::user(input.text.trim()),
                ],
                task: options.task.into(),
                temperature: options.temperature,
                max_tokens: options.max_tokens,
            })?;
            if result.content.trim().is_empty() {
                return Err(Error::InvalidStructuredOutput {
                    contract: crate::CREATOR_CONTENT_SCHEMA_V1.to_owned(),
                    attempts: 1,
                    reason: format!(
                        "{} returned an empty creator script",
                        provider.provider_id_v1()
                    ),
                });
            }
            Ok(result.content.trim().to_owned())
        }
    }
}

fn create_scene_with_provider_v1(
    provider: &dyn LlmProviderV1,
    segment: &SegmentV1,
    scene_id: &str,
    options: &crate::CreatorContentSceneOptionsV1,
) -> Result<SceneIntentV1> {
    options.validate_v1()?;
    let generation = SceneIntentGenerationOptions {
        aspect_ratio: options.aspect_ratio.clone(),
        model: None,
        max_attempts: 3,
        max_tokens: Some(1_200),
        visual_rules: options.visual_rules.clone(),
        avoid: options.avoid.clone(),
    };
    let messages = build_scene_intent_messages_v1(segment, scene_id, &generation)?;
    let request = StructuredLlmRequestOptionsV1 {
        contract: format!("{SCENE_INTENT_SCHEMA}.v{SCENE_INTENT_SCHEMA_VERSION}"),
        model: generation.model.clone(),
        task: LlmTaskV1::Reasoning,
        temperature: Some(0.2),
        max_tokens: generation.max_tokens,
        max_attempts: generation.max_attempts,
    };
    chat_structured_with_provider_v1(provider, messages, &request, |scene: &SceneIntentV1| {
        validate_generated_scene_intent(scene, segment, scene_id, &generation)
    })
}

fn build_scene_intent_messages_v1(
    segment: &SegmentV1,
    scene_id: &str,
    options: &SceneIntentGenerationOptions,
) -> Result<Vec<LlmMessage>> {
    segment.validate_v1()?;
    options.validate()?;
    if scene_id.trim().is_empty() {
        return Err(Error::InvalidContract(
            "scene id must not be empty".to_owned(),
        ));
    }
    let input = json!({
        "scene_id": scene_id,
        "segment": segment,
        "aspect_ratio": options.aspect_ratio,
        "visual_rules": options.visual_rules,
        "required_avoid": options.avoid,
    });
    let input_json = serde_json::to_string_pretty(&input)?;
    Ok(vec![
        LlmMessage::system("You are OmniCreator Scene Intelligence. Translate narration meaning into a provider-neutral visual intent. Do not choose providers, accounts, URLs, or physical model routes. Avoid repetitive religious stock cliches unless the narration specifically requires them."),
        LlmMessage::user(format!(
            "Create exactly one SceneIntent v1 object for the supplied segment.\nThe output must use schema '{SCENE_INTENT_SCHEMA}' with schema_version {SCENE_INTENT_SCHEMA_VERSION}.\nPreserve scene_id, segment_id, narration, and aspect_ratio exactly from the input.\nChoose scene_type from literal, emotional, or conceptual.\nProvide at least {MIN_SCENE_VISUAL_IDEAS_V1} distinct visual_ideas.\nProvide {MIN_SCENE_SEARCH_QUERIES_V1} to {MAX_SCENE_SEARCH_QUERIES_V1} distinct search_queries that describe concrete shootable or searchable visuals, not spiritual abstractions or keyword soup.\nCarry every required_avoid rule into the output avoid list.\nPrefer metaphor and emotional specificity when literal religious stock imagery would be repetitive.\nReturn JSON only.\nInput:\n{input_json}"
        )),
    ])
}

fn gateway_model_v1(provider_id: &str, model: LlmGatewayModel) -> Result<LlmProviderModelV1> {
    if model.id.trim().is_empty() {
        return Err(Error::InvalidLlmProviderResponse {
            provider: provider_id.to_owned(),
            message: "model discovery returned an empty id".to_owned(),
        });
    }
    let display_name = model
        .llmgateway
        .as_ref()
        .and_then(|metadata| metadata.display_name.clone())
        .unwrap_or_else(|| model.id.clone());
    Ok(LlmProviderModelV1 {
        id: model.id,
        display_name,
        provider_id: provider_id.to_owned(),
    })
}

fn decode_provider_http_v1(
    provider: &str,
    response: std::result::Result<ureq::Response, ureq::Error>,
) -> Result<Value> {
    match response {
        Ok(response) => {
            let status = response.status();
            let body = response.into_string()?;
            serde_json::from_str(&body).map_err(|error| Error::InvalidLlmProviderResponse {
                provider: provider.to_owned(),
                message: format!("HTTP {status} returned invalid JSON: {error}"),
            })
        }
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            let parsed = serde_json::from_str::<Value>(&body).ok();
            let error = parsed.as_ref().and_then(|value| value.get("error"));
            let message = error
                .and_then(|value| value.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("request failed")
                .to_owned();
            let code = error
                .and_then(|value| value.get("code").or_else(|| value.get("type")))
                .and_then(value_as_text_v1);
            Err(Error::LlmProviderApi {
                provider: provider.to_owned(),
                status,
                code,
                message,
            })
        }
        Err(ureq::Error::Transport(error)) => Err(Error::LlmProviderTransport {
            provider: provider.to_owned(),
            message: error.to_string(),
        }),
    }
}

fn parse_provider_chat_result_v1(provider: &str, value: Value) -> Result<LlmChatResult> {
    #[derive(Deserialize)]
    struct Completion {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        model: Option<String>,
        choices: Vec<Choice>,
    }
    #[derive(Deserialize)]
    struct Choice {
        message: Message,
    }
    #[derive(Deserialize)]
    struct Message {
        content: Option<String>,
    }
    let response: Completion =
        serde_json::from_value(value).map_err(|error| Error::InvalidLlmProviderResponse {
            provider: provider.to_owned(),
            message: format!("invalid chat response: {error}"),
        })?;
    let content = response
        .choices
        .into_iter()
        .find_map(|choice| choice.message.content)
        .filter(|content| !content.trim().is_empty())
        .ok_or_else(|| Error::InvalidLlmProviderResponse {
            provider: provider.to_owned(),
            message: "chat response does not contain assistant text".to_owned(),
        })?;
    Ok(LlmChatResult {
        id: response.id,
        model: response.model,
        content,
    })
}

fn parse_provider_models_v1(provider: &str, value: Value) -> Result<Vec<LlmProviderModelV1>> {
    let data = value.get("data").and_then(Value::as_array).ok_or_else(|| {
        Error::InvalidLlmProviderResponse {
            provider: provider.to_owned(),
            message: "models response does not contain a data array".to_owned(),
        }
    })?;
    data.iter()
        .map(|model| {
            let id = model
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| Error::InvalidLlmProviderResponse {
                    provider: provider.to_owned(),
                    message: "model discovery returned an empty id".to_owned(),
                })?;
            let display_name = model
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(id);
            Ok(LlmProviderModelV1 {
                id: id.to_owned(),
                display_name: display_name.to_owned(),
                provider_id: provider.to_owned(),
            })
        })
        .collect()
}

fn value_as_text_v1(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn decode_structured_output_v1<T, F>(content: &str, validate: &F) -> std::result::Result<T, String>
where
    T: DeserializeOwned,
    F: Fn(&T) -> Result<()>,
{
    let json = extract_json_value_v1(content)?;
    let value =
        serde_json::from_str::<T>(json).map_err(|error| format!("JSON decode failed: {error}"))?;
    validate(&value).map_err(|error| format!("contract validation failed: {error}"))?;
    Ok(value)
}

fn extract_json_value_v1(content: &str) -> std::result::Result<&str, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("response is empty".to_owned());
    }
    let source = strip_json_fence_v1(trimmed).unwrap_or(trimmed).trim();
    let start = source
        .char_indices()
        .find_map(|(index, ch)| matches!(ch, '{' | '[').then_some(index))
        .ok_or_else(|| "response does not contain a JSON object or array".to_owned())?;
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for (relative_index, ch) in source[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                let expected = stack
                    .pop()
                    .ok_or_else(|| "JSON structure closes before it opens".to_owned())?;
                if ch != expected {
                    return Err(format!(
                        "JSON structure is mismatched: expected {expected}, found {ch}"
                    ));
                }
                if stack.is_empty() {
                    let end = start + relative_index + ch.len_utf8();
                    return Ok(source[start..end].trim());
                }
            }
            _ => {}
        }
    }
    Err("JSON object or array is incomplete".to_owned())
}

fn strip_json_fence_v1(content: &str) -> Option<&str> {
    let rest = content.strip_prefix("```")?;
    let header_end = rest.find('\n')?;
    let language = rest[..header_end].trim();
    if !language.is_empty() && !language.eq_ignore_ascii_case("json") {
        return None;
    }
    let fenced_body = &rest[header_end + 1..];
    let close = fenced_body.rfind("```")?;
    if !fenced_body[close + 3..].trim().is_empty() {
        return None;
    }
    Some(fenced_body[..close].trim())
}

fn structured_repair_prompt_v1(contract: &str, reason: &str) -> String {
    format!(
        "Repair the previous response so it satisfies structured-output contract '{contract}'. Return exactly one valid JSON object or array and no Markdown or prose. Preserve the intended meaning. Validation error: {reason}"
    )
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        thread,
    };

    use super::*;

    fn with_env_var<T>(name: &str, value: &str, operation: impl FnOnce() -> T) -> T {
        let previous = env::var(name).ok();
        env::set_var(name, value);
        let result = operation();
        if let Some(previous) = previous {
            env::set_var(name, previous);
        } else {
            env::remove_var(name);
        }
        result
    }

    #[test]
    fn openrouter_profile_uses_official_openai_compatible_defaults_without_secret_value() {
        let config = LlmProviderConfigV1::openrouter_v1();
        config.validate_v1().unwrap();
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(
            json["provider"]["config"]["base_url"],
            DEFAULT_OPENROUTER_BASE_URL_V1
        );
        assert_eq!(
            json["provider"]["config"]["api_key_env"],
            DEFAULT_OPENROUTER_API_KEY_ENV_V1
        );
        assert_eq!(
            json["provider"]["config"]["default_model"],
            DEFAULT_OPENROUTER_MODEL_V1
        );
        assert!(json.to_string().find("sk-or-").is_none());
    }

    #[test]
    fn machine_local_provider_config_round_trips_without_api_key_field() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("llm-provider.json");
        let config = LlmProviderConfigV1::openrouter_v1();
        config.save(&path).unwrap();
        let loaded = LlmProviderConfigV1::load(&path).unwrap();
        assert_eq!(loaded, config);
        let raw = fs::read_to_string(path).unwrap();
        assert!(!raw.contains("\"api_key\""));
    }

    #[test]
    fn openai_compatible_chat_uses_standard_endpoint_and_omits_llmgateway_fields() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let captured = Arc::new(Mutex::new(String::new()));
        let server_capture = Arc::clone(&captured);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = vec![0_u8; 16_384];
            let read = stream.read(&mut buffer).unwrap();
            *server_capture.lock().unwrap() = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let body = r#"{"id":"chat-1","model":"openrouter/auto","choices":[{"message":{"role":"assistant","content":"Creator script"}}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let config = OpenAiCompatibleConfigV1 {
            base_url: format!("http://{address}/api/v1"),
            ..OpenAiCompatibleConfigV1::openrouter_v1()
        };
        let provider = OpenAiCompatibleProviderV1::new(config).unwrap();
        let result = with_env_var(DEFAULT_OPENROUTER_API_KEY_ENV_V1, "test-secret", || {
            provider
                .chat_v1(&LlmProviderChatRequestV1 {
                    model: None,
                    messages: vec![LlmMessage::user("Create content")],
                    task: LlmTaskV1::Reasoning,
                    temperature: Some(0.1),
                    max_tokens: Some(500),
                })
                .unwrap()
        });
        server.join().unwrap();
        assert_eq!(result.content, "Creator script");
        let request = captured.lock().unwrap();
        assert!(request.starts_with("POST /api/v1/chat/completions "));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer test-secret"));
        assert!(request.contains("\"model\":\"openrouter/auto\""));
        assert!(!request.contains("llmgateway_task"));
        assert!(!request.contains("/_llmgateway/"));
    }

    #[test]
    fn provider_error_body_is_normalized_without_debug_payload() {
        let error = decode_provider_http_v1(
            "openrouter",
            Err(ureq::Error::Status(
                429,
                ureq::Response::new(
                    429,
                    "Too Many Requests",
                    r#"{"error":{"code":"rate_limit","message":"slow down"},"debug":"private"}"#,
                )
                .unwrap(),
            )),
        )
        .unwrap_err();
        match error {
            Error::LlmProviderApi {
                provider,
                status,
                code,
                message,
            } => {
                assert_eq!(provider, "openrouter");
                assert_eq!(status, 429);
                assert_eq!(code.as_deref(), Some("rate_limit"));
                assert_eq!(message, "slow down");
                assert!(!message.contains("private"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn readiness_exposes_env_name_and_boolean_not_secret() {
        let config = OpenAiCompatibleConfigV1::openrouter_v1();
        let provider = OpenAiCompatibleProviderV1::new(config).unwrap();
        let readiness = with_env_var(DEFAULT_OPENROUTER_API_KEY_ENV_V1, "top-secret", || {
            provider.readiness_v1()
        });
        let json = serde_json::to_string(&readiness).unwrap();
        assert!(readiness.credential_present);
        assert!(json.contains(DEFAULT_OPENROUTER_API_KEY_ENV_V1));
        assert!(!json.contains("top-secret"));
    }
}
