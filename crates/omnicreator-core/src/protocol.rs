use serde::{Deserialize, Serialize};

use crate::{ir::validate_schema, Error, Result};

pub const PLUGIN_MANIFEST_SCHEMA: &str = "omnicreator.plugin-manifest";
pub const PLUGIN_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const PLUGIN_API_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManifest {
    pub schema: String,
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
    #[serde(default)]
    pub types: Vec<String>,
    pub entrypoint: PluginEntrypoint,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub scene_types: Vec<String>,
    #[serde(default)]
    pub permissions: PluginPermissions,
    pub settings: Option<PluginSettings>,
    #[serde(default)]
    pub resources: Option<ComputeRequirements>,
}

impl PluginManifest {
    pub fn validate_v1(&self) -> Result<()> {
        validate_schema(
            &self.schema,
            self.schema_version,
            PLUGIN_MANIFEST_SCHEMA,
            PLUGIN_MANIFEST_SCHEMA_VERSION,
        )?;
        if self.api_version != PLUGIN_API_VERSION {
            return Err(Error::InvalidContract(format!(
                "unsupported plugin api_version {}; expected {}",
                self.api_version, PLUGIN_API_VERSION
            )));
        }
        require_non_empty("plugin id", &self.id)?;
        require_non_empty("plugin name", &self.name)?;
        require_non_empty("plugin version", &self.version)?;
        require_non_empty("plugin entrypoint command", &self.entrypoint.command)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginEntrypoint {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginPermissions {
    #[serde(default)]
    pub filesystem: Vec<String>,
    #[serde(default)]
    pub network: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginSettings {
    pub schema: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeRequirements {
    pub gpu: ResourceRequirement,
    pub min_vram_mb: Option<u64>,
    pub model_group: Option<String>,
    pub parallelizable: bool,
    pub cost_metric: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResourceRequirement {
    Required,
    Optional,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginRequest {
    pub api_version: u32,
    pub request_id: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

impl PluginRequest {
    pub fn validate_v1(&self) -> Result<()> {
        validate_api_version(self.api_version)?;
        require_non_empty("request_id", &self.request_id)?;
        require_non_empty("plugin method", &self.method)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum PluginResponse {
    Success {
        api_version: u32,
        request_id: String,
        result: serde_json::Value,
    },
    Failure {
        api_version: u32,
        request_id: String,
        error: PluginError,
    },
}

impl PluginResponse {
    pub fn validate_v1(&self) -> Result<()> {
        match self {
            Self::Success {
                api_version,
                request_id,
                ..
            }
            | Self::Failure {
                api_version,
                request_id,
                ..
            } => {
                validate_api_version(*api_version)?;
                require_non_empty("response request_id", request_id)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub retry_after_seconds: Option<u64>,
    pub suggested_fallback: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginProgressEvent {
    pub api_version: u32,
    pub event: String,
    pub request_id: String,
    pub progress: PluginProgress,
}

impl PluginProgressEvent {
    pub fn validate_v1(&self) -> Result<()> {
        validate_api_version(self.api_version)?;
        if self.event != "progress" {
            return Err(Error::InvalidContract(format!(
                "unsupported plugin event {}; expected progress",
                self.event
            )));
        }
        require_non_empty("event request_id", &self.request_id)?;
        if self.progress.percent > 100 {
            return Err(Error::InvalidContract(
                "plugin progress percent must be <= 100".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginProgress {
    pub percent: u8,
    pub message: Option<String>,
}

fn validate_api_version(api_version: u32) -> Result<()> {
    if api_version != PLUGIN_API_VERSION {
        return Err(Error::InvalidContract(format!(
            "unsupported plugin api_version {api_version}; expected {PLUGIN_API_VERSION}"
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
    use super::*;

    #[test]
    fn incompatible_plugin_api_is_rejected() {
        let request = PluginRequest {
            api_version: 2,
            request_id: "req_1".to_owned(),
            method: "plugin.health".to_owned(),
            params: serde_json::Value::Null,
        };

        assert!(matches!(
            request.validate_v1(),
            Err(Error::InvalidContract(_))
        ));
    }
}
