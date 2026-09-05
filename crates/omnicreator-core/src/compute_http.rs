use std::{env, fs, io::copy, path::Path, time::Duration};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{
    ComputeJobDispatchAckV1, ComputeJobDispatchV1, ComputeProvider, ComputeProviderCapabilitiesV1,
    ComputeProviderConnectV1, ComputeProviderExecution, ComputeProviderHeartbeatV1,
    ComputeProviderMetadataV1, ComputeRemoteJournalEntryV1, Error, Result,
};

pub const COMPUTE_HTTP_PROTOCOL_V1: &str = "omnicreator.compute-http";
const CONNECT_PATH: &str = "/v1/compute/connect";
const DISCONNECT_PATH: &str = "/v1/compute/disconnect";
const HEARTBEAT_PATH: &str = "/v1/compute/heartbeat";
const CAPABILITIES_PATH: &str = "/v1/compute/capabilities";
const DISPATCH_PATH: &str = "/v1/compute/dispatch";
const JOURNAL_PATH: &str = "/v1/compute/journal";
const ARTIFACT_PATH: &str = "/v1/compute/artifact";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpComputeProviderConfigV1 {
    pub provider_id: String,
    pub base_url: String,
    pub bearer_token_env: Option<String>,
    pub timeout_seconds: u64,
}

impl HttpComputeProviderConfigV1 {
    pub fn validate_v1(&self) -> Result<()> {
        require_identifier("HTTP compute provider_id", &self.provider_id)?;
        let base_url = self.base_url.trim();
        if base_url != self.base_url
            || !(base_url.starts_with("http://") || base_url.starts_with("https://"))
            || base_url.trim_end_matches('/').is_empty()
        {
            return Err(Error::InvalidComputeProviderConfig(
                "base_url must be a normalized http:// or https:// URL".to_owned(),
            ));
        }
        if self
            .bearer_token_env
            .as_deref()
            .is_some_and(|value| value.trim().is_empty() || value.trim() != value)
        {
            return Err(Error::InvalidComputeProviderConfig(
                "bearer_token_env must be a normalized non-empty environment variable name"
                    .to_owned(),
            ));
        }
        if !(1..=600).contains(&self.timeout_seconds) {
            return Err(Error::InvalidComputeProviderConfig(
                "timeout_seconds must be between 1 and 600".to_owned(),
            ));
        }
        Ok(())
    }

    fn endpoint(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    fn bearer_token(&self) -> Result<Option<String>> {
        let Some(variable) = self.bearer_token_env.as_deref() else {
            return Ok(None);
        };
        let token = env::var(variable)
            .map_err(|_| Error::MissingComputeProviderCredential(variable.to_owned()))?;
        if token.trim().is_empty() {
            return Err(Error::MissingComputeProviderCredential(variable.to_owned()));
        }
        Ok(Some(token))
    }
}

pub struct HttpComputeProvider {
    config: HttpComputeProviderConfigV1,
    agent: ureq::Agent,
}

impl HttpComputeProvider {
    pub fn new(config: HttpComputeProviderConfigV1) -> Result<Self> {
        config.validate_v1()?;
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build();
        Ok(Self { config, agent })
    }

    pub fn config(&self) -> &HttpComputeProviderConfigV1 {
        &self.config
    }

    fn request(&self, path: &str) -> Result<ureq::Request> {
        let mut request = self
            .agent
            .post(&self.config.endpoint(path))
            .set("Content-Type", "application/json")
            .set("X-OmniCreator-Compute-Protocol", COMPUTE_HTTP_PROTOCOL_V1);
        if let Some(token) = self.config.bearer_token()? {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        Ok(request)
    }

    fn post_json<T, R>(&self, path: &str, body: &T) -> Result<R>
    where
        T: Serialize,
        R: DeserializeOwned,
    {
        let payload = serde_json::to_string(body)?;
        let response = self.request(path)?.send_string(&payload);
        decode_json_response(response)
    }

    fn post_binary<T>(&self, path: &str, body: &T, destination: &Path) -> Result<()>
    where
        T: Serialize,
    {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let payload = serde_json::to_string(body)?;
        let response = self.request(path)?.send_string(&payload);
        let mut response = decode_binary_response(response)?;
        let mut file = fs::File::create(destination)?;
        copy(&mut response, &mut file)?;
        Ok(())
    }
}

#[derive(Serialize)]
struct ProviderRequest<'a> {
    provider_id: &'a str,
}

#[derive(Serialize)]
struct SessionRequest<'a> {
    provider_id: &'a str,
    session_id: &'a str,
}

#[derive(Serialize)]
struct JournalRequest<'a> {
    provider_id: &'a str,
    session_id: &'a str,
    after_sequence: Option<u64>,
}

#[derive(Serialize)]
struct ArtifactRequest<'a> {
    provider_id: &'a str,
    session_id: &'a str,
    transfer_ref: &'a str,
}

#[derive(Deserialize)]
struct JournalResponse {
    #[serde(default)]
    entries: Vec<ComputeRemoteJournalEntryV1>,
}

impl ComputeProvider for HttpComputeProvider {
    fn metadata(&self) -> ComputeProviderMetadataV1 {
        ComputeProviderMetadataV1 {
            provider_id: self.config.provider_id.clone(),
            display_name: Some("HTTP Compute Worker".to_owned()),
            implementation_version: Some("compute-http-v1".to_owned()),
        }
    }

    fn connect(&mut self) -> Result<ComputeProviderConnectV1> {
        self.post_json(
            CONNECT_PATH,
            &ProviderRequest {
                provider_id: &self.config.provider_id,
            },
        )
    }

    fn disconnect(&mut self, session_id: &str) -> Result<()> {
        let _: serde_json::Value = self.post_json(
            DISCONNECT_PATH,
            &SessionRequest {
                provider_id: &self.config.provider_id,
                session_id,
            },
        )?;
        Ok(())
    }

    fn heartbeat(&mut self, session_id: &str) -> Result<ComputeProviderHeartbeatV1> {
        self.post_json(
            HEARTBEAT_PATH,
            &SessionRequest {
                provider_id: &self.config.provider_id,
                session_id,
            },
        )
    }

    fn discover_capabilities(&mut self, session_id: &str) -> Result<ComputeProviderCapabilitiesV1> {
        self.post_json(
            CAPABILITIES_PATH,
            &SessionRequest {
                provider_id: &self.config.provider_id,
                session_id,
            },
        )
    }
}

impl ComputeProviderExecution for HttpComputeProvider {
    fn dispatch_job(&mut self, dispatch: &ComputeJobDispatchV1) -> Result<ComputeJobDispatchAckV1> {
        self.post_json(DISPATCH_PATH, dispatch)
    }

    fn read_journal(
        &mut self,
        provider_id: &str,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<Vec<ComputeRemoteJournalEntryV1>> {
        let response: JournalResponse = self.post_json(
            JOURNAL_PATH,
            &JournalRequest {
                provider_id,
                session_id,
                after_sequence,
            },
        )?;
        for entry in &response.entries {
            entry.validate_v1()?;
        }
        Ok(response.entries)
    }

    fn transfer_artifact(
        &mut self,
        provider_id: &str,
        session_id: &str,
        transfer_ref: &str,
        destination: &Path,
    ) -> Result<()> {
        require_identifier("artifact provider_id", provider_id)?;
        require_identifier("artifact session_id", session_id)?;
        require_identifier("artifact transfer_ref", transfer_ref)?;
        self.post_binary(
            ARTIFACT_PATH,
            &ArtifactRequest {
                provider_id,
                session_id,
                transfer_ref,
            },
            destination,
        )
    }
}

fn decode_json_response<R>(response: std::result::Result<ureq::Response, ureq::Error>) -> Result<R>
where
    R: DeserializeOwned,
{
    match response {
        Ok(response) => {
            let status = response.status();
            let body = response.into_string()?;
            serde_json::from_str(&body).map_err(|error| {
                Error::InvalidComputeProviderResponse(format!(
                    "HTTP {status} returned invalid JSON: {error}"
                ))
            })
        }
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            Err(compute_api_error(status, &body))
        }
        Err(ureq::Error::Transport(error)) => {
            Err(Error::ComputeProviderTransport(error.to_string()))
        }
    }
}

fn decode_binary_response(
    response: std::result::Result<ureq::Response, ureq::Error>,
) -> Result<Box<dyn std::io::Read + Send + Sync + 'static>> {
    match response {
        Ok(response) => Ok(response.into_reader()),
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            Err(compute_api_error(status, &body))
        }
        Err(ureq::Error::Transport(error)) => {
            Err(Error::ComputeProviderTransport(error.to_string()))
        }
    }
}

fn compute_api_error(status: u16, body: &str) -> Error {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|error| {
            error
                .get("message")
                .and_then(serde_json::Value::as_str)
                .or_else(|| error.as_str())
        })
        .unwrap_or("compute provider request failed")
        .to_owned();
    Error::ComputeProviderApi { status, message }
}

fn require_identifier(label: &str, value: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed != value || value.chars().any(char::is_control) {
        return Err(Error::InvalidComputeProviderConfig(format!(
            "{label} must be a normalized non-empty identifier"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_is_provider_neutral_and_never_contains_a_secret_value() {
        let config = HttpComputeProviderConfigV1 {
            provider_id: "remote-gpu".to_owned(),
            base_url: "http://127.0.0.1:8787".to_owned(),
            bearer_token_env: Some("OMNICREATOR_COMPUTE_TOKEN".to_owned()),
            timeout_seconds: 30,
        };
        config.validate_v1().unwrap();
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("OMNICREATOR_COMPUTE_TOKEN"));
        assert!(!json.contains("Bearer "));
        assert!(!json.contains("kaggle"));
    }

    #[test]
    fn config_rejects_absolute_path_and_non_http_endpoints() {
        let mut config = HttpComputeProviderConfigV1 {
            provider_id: "remote-gpu".to_owned(),
            base_url: "/tmp/worker.sock".to_owned(),
            bearer_token_env: None,
            timeout_seconds: 30,
        };
        assert!(config.validate_v1().is_err());
        config.base_url = "http://127.0.0.1:8787 ".to_owned();
        assert!(config.validate_v1().is_err());
    }
}
