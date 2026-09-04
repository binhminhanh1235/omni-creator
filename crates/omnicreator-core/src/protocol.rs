use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManifest {
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
    pub resources: Option<ComputeRequirements>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginEntrypoint {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub retry_after_seconds: Option<u64>,
    pub suggested_fallback: Option<String>,
}
