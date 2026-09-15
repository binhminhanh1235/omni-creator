use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    ApplicationControlService, ControlErrorCodeV1, ControlErrorV1, ControlResultV1,
    ExternalVisualResultRequestV1, ExternalVoiceResultRequestV1,
};

pub const EXTERNAL_RESULT_BATCH_SCHEMA_V1: &str = "omnicreator.external-result-batch";
pub const EXTERNAL_RESULT_BATCH_VERSION_V1: u32 = 1;
pub const EXTERNAL_RESULT_BATCH_MAX_ITEMS_V1: usize = 256;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalResultBatchKindV1 {
    Visual,
    Voice,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExternalResultBatchItemRequestV1 {
    Visual {
        item_id: String,
        result: Box<ExternalVisualResultRequestV1>,
    },
    Voice {
        item_id: String,
        result: ExternalVoiceResultRequestV1,
    },
}

impl ExternalResultBatchItemRequestV1 {
    fn item_id_v1(&self) -> &str {
        match self {
            Self::Visual { item_id, .. } | Self::Voice { item_id, .. } => item_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExternalResultBatchRequestV1 {
    pub schema: String,
    pub version: u32,
    pub items: Vec<ExternalResultBatchItemRequestV1>,
}

impl ExternalResultBatchRequestV1 {
    pub fn new(items: Vec<ExternalResultBatchItemRequestV1>) -> Self {
        Self {
            schema: EXTERNAL_RESULT_BATCH_SCHEMA_V1.to_owned(),
            version: EXTERNAL_RESULT_BATCH_VERSION_V1,
            items,
        }
    }

    pub fn validate_v1(&self) -> ControlResultV1<()> {
        if self.schema != EXTERNAL_RESULT_BATCH_SCHEMA_V1
            || self.version != EXTERNAL_RESULT_BATCH_VERSION_V1
        {
            return Err(ControlErrorV1::new(
                ControlErrorCodeV1::InvalidInput,
                "unsupported external-result batch schema/version",
            ));
        }
        if self.items.is_empty() || self.items.len() > EXTERNAL_RESULT_BATCH_MAX_ITEMS_V1 {
            return Err(ControlErrorV1::new(
                ControlErrorCodeV1::InvalidInput,
                format!(
                    "external-result batch must contain between 1 and {} items",
                    EXTERNAL_RESULT_BATCH_MAX_ITEMS_V1
                ),
            ));
        }
        let mut seen = BTreeSet::new();
        for item in &self.items {
            let item_id = item.item_id_v1();
            if item_id.trim().is_empty()
                || item_id.len() > 128
                || item_id.chars().any(char::is_control)
            {
                return Err(ControlErrorV1::new(
                    ControlErrorCodeV1::InvalidInput,
                    "external-result batch item_id must be a non-empty portable label up to 128 characters",
                ));
            }
            if !seen.insert(item_id) {
                return Err(ControlErrorV1::new(
                    ControlErrorCodeV1::InvalidInput,
                    format!("duplicate external-result batch item_id: {item_id}"),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalResultBatchItemStatusV1 {
    Committed,
    Reused,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExternalResultBatchItemOutcomeV1 {
    pub item_id: String,
    pub kind: ExternalResultBatchKindV1,
    pub project_id: String,
    pub canonical_unit: String,
    pub request_sha256: String,
    pub status: ExternalResultBatchItemStatusV1,
    pub artifact_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ControlErrorV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExternalResultBatchOutcomeV1 {
    pub schema: String,
    pub version: u32,
    pub total: usize,
    pub committed: usize,
    pub reused: usize,
    pub failed: usize,
    pub items: Vec<ExternalResultBatchItemOutcomeV1>,
}

impl<'a> ApplicationControlService<'a> {
    pub fn provide_external_result_batch_v1(
        &mut self,
        request: &ExternalResultBatchRequestV1,
    ) -> ControlResultV1<ExternalResultBatchOutcomeV1> {
        if self.access_v1().is_read_only() {
            return Err(ControlErrorV1::read_only());
        }
        request.validate_v1()?;

        let mut items = Vec::with_capacity(request.items.len());
        for item in &request.items {
            let outcome = match item {
                ExternalResultBatchItemRequestV1::Visual { item_id, result } => {
                    let project_id = result.request.project_id.clone();
                    let canonical_unit = result.request.scene_id.clone();
                    let request_sha256 = result.request.request_sha256.clone();
                    match self.provide_external_visual_v1(result) {
                        Ok(response) => {
                            let data = response.data;
                            ExternalResultBatchItemOutcomeV1 {
                                item_id: item_id.clone(),
                                kind: ExternalResultBatchKindV1::Visual,
                                project_id,
                                canonical_unit,
                                request_sha256,
                                status: if data.ingestion.cache_hit {
                                    ExternalResultBatchItemStatusV1::Reused
                                } else {
                                    ExternalResultBatchItemStatusV1::Committed
                                },
                                artifact_ids: vec![data.artifact.artifact_id],
                                error: None,
                            }
                        }
                        Err(error) => ExternalResultBatchItemOutcomeV1 {
                            item_id: item_id.clone(),
                            kind: ExternalResultBatchKindV1::Visual,
                            project_id,
                            canonical_unit,
                            request_sha256,
                            status: ExternalResultBatchItemStatusV1::Failed,
                            artifact_ids: Vec::new(),
                            error: Some(error),
                        },
                    }
                }
                ExternalResultBatchItemRequestV1::Voice { item_id, result } => {
                    let project_id = result.request.project_id.clone();
                    let canonical_unit = result.request.segment_id.clone();
                    let request_sha256 = result.request.request_sha256.clone();
                    match self.provide_external_voice_v1(result) {
                        Ok(response) => {
                            let data = response.data;
                            ExternalResultBatchItemOutcomeV1 {
                                item_id: item_id.clone(),
                                kind: ExternalResultBatchKindV1::Voice,
                                project_id,
                                canonical_unit,
                                request_sha256,
                                status: if data.cache_hit {
                                    ExternalResultBatchItemStatusV1::Reused
                                } else {
                                    ExternalResultBatchItemStatusV1::Committed
                                },
                                artifact_ids: vec![
                                    data.audio.artifact_id,
                                    data.timing_artifact.artifact_id,
                                ],
                                error: None,
                            }
                        }
                        Err(error) => ExternalResultBatchItemOutcomeV1 {
                            item_id: item_id.clone(),
                            kind: ExternalResultBatchKindV1::Voice,
                            project_id,
                            canonical_unit,
                            request_sha256,
                            status: ExternalResultBatchItemStatusV1::Failed,
                            artifact_ids: Vec::new(),
                            error: Some(error),
                        },
                    }
                }
            };
            items.push(outcome);
        }

        let committed = items
            .iter()
            .filter(|item| item.status == ExternalResultBatchItemStatusV1::Committed)
            .count();
        let reused = items
            .iter()
            .filter(|item| item.status == ExternalResultBatchItemStatusV1::Reused)
            .count();
        let failed = items
            .iter()
            .filter(|item| item.status == ExternalResultBatchItemStatusV1::Failed)
            .count();

        Ok(ExternalResultBatchOutcomeV1 {
            schema: EXTERNAL_RESULT_BATCH_SCHEMA_V1.to_owned(),
            version: EXTERNAL_RESULT_BATCH_VERSION_V1,
            total: items.len(),
            committed,
            reused,
            failed,
            items,
        })
    }
}
