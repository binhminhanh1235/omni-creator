use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    deterministic_input_hash, ComputeRequirements, Error, GpuJobPreparationV1, LogicalUri,
    ResourceRequirement, Result, SegmentV1, VoiceDirectionV1,
};

pub const SEGMENT_TTS_INPUT_SCHEMA_V1: &str = "omnicreator.segment-tts-input";
pub const SEGMENT_TTS_NORMALIZATION_V1: &str = "whitespace-v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct PronunciationRuleV1 {
    pub written: String,
    pub pronunciation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceIdentityV1 {
    pub voice_id: String,
    pub voice_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceModelIdentityV1 {
    pub model_id: String,
    pub model_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SegmentTtsProductionInputV1 {
    pub schema: String,
    pub version: u32,
    pub source_text: String,
    pub normalized_text: String,
    pub normalization_version: String,
    pub voice_direction: VoiceDirectionV1,
    #[serde(default)]
    pub pronunciation_rules: Vec<PronunciationRuleV1>,
    pub voice: VoiceIdentityV1,
    pub model: VoiceModelIdentityV1,
    pub settings_fingerprint: String,
}

impl SegmentTtsProductionInputV1 {
    pub fn from_segment_v1(
        segment: &SegmentV1,
        pronunciation_rules: Vec<PronunciationRuleV1>,
        voice: VoiceIdentityV1,
        model: VoiceModelIdentityV1,
        settings_fingerprint: impl Into<String>,
    ) -> Self {
        Self {
            schema: SEGMENT_TTS_INPUT_SCHEMA_V1.to_owned(),
            version: 1,
            source_text: segment.text.clone(),
            normalized_text: normalize_segment_text_v1(&segment.text),
            normalization_version: SEGMENT_TTS_NORMALIZATION_V1.to_owned(),
            voice_direction: segment.voice_direction.clone(),
            pronunciation_rules,
            voice,
            model,
            settings_fingerprint: settings_fingerprint.into(),
        }
    }

    pub fn validate_hashable_v1(&self) -> Result<()> {
        if self.schema != SEGMENT_TTS_INPUT_SCHEMA_V1 || self.version != 1 {
            return Err(Error::InvalidContract(
                "unsupported segment TTS production input schema/version".to_owned(),
            ));
        }
        require_identifier("segment TTS source_text", &self.source_text)?;
        require_identifier("segment TTS normalized_text", &self.normalized_text)?;
        if self.normalized_text != normalize_segment_text_v1(&self.source_text) {
            return Err(Error::InvalidContract(
                "segment TTS normalized_text is stale for the current source_text".to_owned(),
            ));
        }
        require_identifier(
            "segment TTS normalization_version",
            &self.normalization_version,
        )?;
        require_identifier("segment TTS voice_id", &self.voice.voice_id)?;
        require_identifier(
            "segment TTS voice_version",
            &self.voice.voice_version,
        )?;
        require_identifier("segment TTS model_id", &self.model.model_id)?;
        require_identifier(
            "segment TTS model_version",
            &self.model.model_version,
        )?;
        require_identifier(
            "segment TTS settings_fingerprint",
            &self.settings_fingerprint,
        )?;
        validate_pronunciation_rules_v1(&self.pronunciation_rules)?;
        validate_voice_direction_v1(&self.voice_direction)
    }

    pub fn input_hash_v1(&self) -> Result<String> {
        self.validate_hashable_v1()?;

        let mut parts = vec![
            b"segment-tts-v1".to_vec(),
            self.normalization_version.as_bytes().to_vec(),
            self.normalized_text.as_bytes().to_vec(),
            self.voice.voice_id.as_bytes().to_vec(),
            self.voice.voice_version.as_bytes().to_vec(),
            self.model.model_id.as_bytes().to_vec(),
            self.model.model_version.as_bytes().to_vec(),
            self.settings_fingerprint.as_bytes().to_vec(),
            option_bytes(self.voice_direction.tone.as_deref()),
            option_bytes(self.voice_direction.pace.as_deref()),
        ];

        parts.push(self.voice_direction.tags.len().to_string().into_bytes());
        for tag in &self.voice_direction.tags {
            parts.push(tag.as_bytes().to_vec());
        }

        let rules = canonical_pronunciation_rules_v1(&self.pronunciation_rules)?;
        parts.push(rules.len().to_string().into_bytes());
        for rule in rules {
            parts.push(rule.written.as_bytes().to_vec());
            parts.push(rule.pronunciation.as_bytes().to_vec());
        }

        let refs = parts.iter().map(Vec::as_slice).collect::<Vec<_>>();
        Ok(deterministic_input_hash(&refs))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentTtsLockStateV1 {
    pub normalization_locked: bool,
    pub pronunciation_locked: bool,
}

impl SegmentTtsLockStateV1 {
    pub fn input_immutable(&self) -> bool {
        self.normalization_locked && self.pronunciation_locked
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentTtsExecutionTargetV1 {
    pub plugin_id: Option<String>,
    pub provider_id: Option<String>,
    pub output_uri: Option<LogicalUri>,
    pub approval_required: bool,
    pub approval_complete: bool,
    pub production_lock_required: bool,
    pub gpu_execution_requested: bool,
    pub requirements: ComputeRequirements,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SegmentTtsPreflightIssueCodeV1 {
    SourceTextMissing,
    NormalizedTextMissing,
    NormalizationVersionMissing,
    NormalizationStale,
    NormalizationUnlocked,
    PronunciationUnlocked,
    PronunciationRuleInvalid,
    PronunciationConflict,
    VoiceMissing,
    VoiceVersionMissing,
    ModelMissing,
    ModelVersionMissing,
    SettingsMissing,
    PluginMissing,
    ProviderMissing,
    OutputMissing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentTtsPreflightIssueV1 {
    pub code: SegmentTtsPreflightIssueCodeV1,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SegmentTtsPreflightStatusV1 {
    Ready,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentTtsPreflightV1 {
    pub status: SegmentTtsPreflightStatusV1,
    #[serde(default)]
    pub issues: Vec<SegmentTtsPreflightIssueV1>,
}

impl SegmentTtsPreflightV1 {
    pub fn is_ready(&self) -> bool {
        self.status == SegmentTtsPreflightStatusV1::Ready
    }

    pub fn has(&self, code: SegmentTtsPreflightIssueCodeV1) -> bool {
        self.issues.iter().any(|issue| issue.code == code)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SegmentTtsPreparationV1 {
    pub segment_id: String,
    pub production_input: SegmentTtsProductionInputV1,
    pub locks: SegmentTtsLockStateV1,
    pub execution: SegmentTtsExecutionTargetV1,
}

impl SegmentTtsPreparationV1 {
    pub fn preflight_v1(&self) -> Result<SegmentTtsPreflightV1> {
        require_identifier("segment TTS segment_id", &self.segment_id)?;
        self.execution.requirements.validate_scheduling_v1()?;
        Ok(evaluate_segment_tts_preflight_v1(
            &self.production_input,
            &self.locks,
            &self.execution,
        ))
    }

    pub fn input_hash_v1(&self) -> Result<String> {
        self.production_input.input_hash_v1()
    }

    pub fn to_gpu_job_preparation_v1(&self, job_id: &str) -> Result<GpuJobPreparationV1> {
        require_identifier("segment TTS job_id", job_id)?;
        let preflight = self.preflight_v1()?;
        let input_hashable = self.production_input.validate_hashable_v1().is_ok();

        Ok(GpuJobPreparationV1 {
            job_id: job_id.to_owned(),
            input_resolved: input_hashable,
            input_immutable: input_hashable && self.locks.input_immutable(),
            plugin_id: normalized_option(self.execution.plugin_id.as_deref()),
            provider_id: normalized_option(self.execution.provider_id.as_deref()),
            model_id: normalized_option(Some(&self.production_input.model.model_id)),
            model_version: normalized_option(Some(
                &self.production_input.model.model_version,
            )),
            settings_fingerprint: normalized_option(Some(
                &self.production_input.settings_fingerprint,
            )),
            output_uri: self.execution.output_uri.clone(),
            approval_required: self.execution.approval_required,
            approval_complete: self.execution.approval_complete,
            production_lock_required: self.execution.production_lock_required,
            preflight_required: true,
            preflight_complete: preflight.is_ready(),
            gpu_execution_requested: self.execution.gpu_execution_requested,
            requirements: self.execution.requirements.clone(),
        })
    }
}

pub fn normalize_segment_text_v1(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn evaluate_segment_tts_preflight_v1(
    input: &SegmentTtsProductionInputV1,
    locks: &SegmentTtsLockStateV1,
    execution: &SegmentTtsExecutionTargetV1,
) -> SegmentTtsPreflightV1 {
    let mut issues = Vec::new();

    if input.source_text.trim().is_empty() {
        push_issue(
            &mut issues,
            SegmentTtsPreflightIssueCodeV1::SourceTextMissing,
            "The segment has no source narration text.",
        );
    }
    if input.normalized_text.trim().is_empty() {
        push_issue(
            &mut issues,
            SegmentTtsPreflightIssueCodeV1::NormalizedTextMissing,
            "Normalized narration text is empty.",
        );
    }
    if input.normalization_version.trim().is_empty() {
        push_issue(
            &mut issues,
            SegmentTtsPreflightIssueCodeV1::NormalizationVersionMissing,
            "The normalization contract version is unknown.",
        );
    }
    if !input.source_text.trim().is_empty()
        && !input.normalized_text.trim().is_empty()
        && input.normalized_text != normalize_segment_text_v1(&input.source_text)
    {
        push_issue(
            &mut issues,
            SegmentTtsPreflightIssueCodeV1::NormalizationStale,
            "Source narration changed after normalization; normalize and lock it again.",
        );
    }
    if !locks.normalization_locked {
        push_issue(
            &mut issues,
            SegmentTtsPreflightIssueCodeV1::NormalizationUnlocked,
            "Normalized narration must be locked before GPU generation.",
        );
    }
    if !locks.pronunciation_locked {
        push_issue(
            &mut issues,
            SegmentTtsPreflightIssueCodeV1::PronunciationUnlocked,
            "Pronunciation rules must be locked before GPU generation.",
        );
    }

    let mut seen_written = BTreeMap::<String, String>::new();
    for rule in &input.pronunciation_rules {
        if rule.written.trim().is_empty() || rule.pronunciation.trim().is_empty() {
            push_issue_once(
                &mut issues,
                SegmentTtsPreflightIssueCodeV1::PronunciationRuleInvalid,
                "Pronunciation rules require both written text and a pronunciation.",
            );
            continue;
        }
        let key = canonical_written_key(&rule.written);
        if let Some(previous) = seen_written.get(&key) {
            if previous != rule.pronunciation.trim() {
                push_issue_once(
                    &mut issues,
                    SegmentTtsPreflightIssueCodeV1::PronunciationConflict,
                    "The same written form has conflicting pronunciation rules.",
                );
            }
        } else {
            seen_written.insert(key, rule.pronunciation.trim().to_owned());
        }
    }

    if input.voice.voice_id.trim().is_empty() {
        push_issue(
            &mut issues,
            SegmentTtsPreflightIssueCodeV1::VoiceMissing,
            "A voice must be selected before TTS generation.",
        );
    }
    if input.voice.voice_version.trim().is_empty() {
        push_issue(
            &mut issues,
            SegmentTtsPreflightIssueCodeV1::VoiceVersionMissing,
            "The selected voice must have an immutable version.",
        );
    }
    if input.model.model_id.trim().is_empty() {
        push_issue(
            &mut issues,
            SegmentTtsPreflightIssueCodeV1::ModelMissing,
            "A voice model must be selected before TTS generation.",
        );
    }
    if input.model.model_version.trim().is_empty() {
        push_issue(
            &mut issues,
            SegmentTtsPreflightIssueCodeV1::ModelVersionMissing,
            "The selected voice model must have an immutable version.",
        );
    }
    if input.settings_fingerprint.trim().is_empty() {
        push_issue(
            &mut issues,
            SegmentTtsPreflightIssueCodeV1::SettingsMissing,
            "Voice generation settings must be resolved before preflight completes.",
        );
    }
    if normalized_option(execution.plugin_id.as_deref()).is_none() {
        push_issue(
            &mut issues,
            SegmentTtsPreflightIssueCodeV1::PluginMissing,
            "A VoiceProvider plugin must be selected before TTS generation.",
        );
    }
    if normalized_option(execution.provider_id.as_deref()).is_none() {
        push_issue(
            &mut issues,
            SegmentTtsPreflightIssueCodeV1::ProviderMissing,
            "A ComputeProvider must be selected before GPU generation.",
        );
    }
    if execution.output_uri.is_none() {
        push_issue(
            &mut issues,
            SegmentTtsPreflightIssueCodeV1::OutputMissing,
            "The logical output destination must be known before generation.",
        );
    }

    issues.sort_by_key(|issue| issue.code);
    SegmentTtsPreflightV1 {
        status: if issues.is_empty() {
            SegmentTtsPreflightStatusV1::Ready
        } else {
            SegmentTtsPreflightStatusV1::Blocked
        },
        issues,
    }
}

pub fn canonical_pronunciation_rules_v1(
    rules: &[PronunciationRuleV1],
) -> Result<Vec<PronunciationRuleV1>> {
    validate_pronunciation_rules_v1(rules)?;
    let mut canonical = rules
        .iter()
        .map(|rule| PronunciationRuleV1 {
            written: rule.written.trim().to_owned(),
            pronunciation: rule.pronunciation.trim().to_owned(),
        })
        .collect::<Vec<_>>();
    canonical.sort_by(|left, right| {
        canonical_written_key(&left.written)
            .cmp(&canonical_written_key(&right.written))
            .then(left.pronunciation.cmp(&right.pronunciation))
    });
    canonical.dedup();
    Ok(canonical)
}

fn validate_pronunciation_rules_v1(rules: &[PronunciationRuleV1]) -> Result<()> {
    let mut seen = BTreeMap::<String, String>::new();
    for rule in rules {
        require_identifier("pronunciation written form", &rule.written)?;
        require_identifier("pronunciation value", &rule.pronunciation)?;
        let key = canonical_written_key(&rule.written);
        let pronunciation = rule.pronunciation.trim().to_owned();
        if let Some(previous) = seen.get(&key) {
            if previous != &pronunciation {
                return Err(Error::InvalidContract(format!(
                    "conflicting pronunciation rules for {}",
                    rule.written.trim()
                )));
            }
        } else {
            seen.insert(key, pronunciation);
        }
    }
    Ok(())
}

fn validate_voice_direction_v1(direction: &VoiceDirectionV1) -> Result<()> {
    if direction
        .tone
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(Error::InvalidContract(
            "voice direction tone must not be blank when present".to_owned(),
        ));
    }
    if direction
        .pace
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(Error::InvalidContract(
            "voice direction pace must not be blank when present".to_owned(),
        ));
    }
    if direction.tags.iter().any(|tag| tag.trim().is_empty()) {
        return Err(Error::InvalidContract(
            "voice direction tags must not contain blank values".to_owned(),
        ));
    }
    let unique = direction
        .tags
        .iter()
        .map(|tag| tag.trim())
        .collect::<BTreeSet<_>>();
    if unique.len() != direction.tags.len() {
        return Err(Error::InvalidContract(
            "voice direction tags must not contain duplicates".to_owned(),
        ));
    }
    Ok(())
}

fn canonical_written_key(value: &str) -> String {
    normalize_segment_text_v1(value).to_lowercase()
}

fn normalized_option(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn option_bytes(value: Option<&str>) -> Vec<u8> {
    value.unwrap_or_default().trim().as_bytes().to_vec()
}

fn push_issue(
    issues: &mut Vec<SegmentTtsPreflightIssueV1>,
    code: SegmentTtsPreflightIssueCodeV1,
    message: impl Into<String>,
) {
    issues.push(SegmentTtsPreflightIssueV1 {
        code,
        message: message.into(),
    });
}

fn push_issue_once(
    issues: &mut Vec<SegmentTtsPreflightIssueV1>,
    code: SegmentTtsPreflightIssueCodeV1,
    message: impl Into<String>,
) {
    if !issues.iter().any(|issue| issue.code == code) {
        push_issue(issues, code, message);
    }
}

fn require_identifier(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidContract(format!("{label} must not be empty")));
    }
    Ok(())
}

pub fn default_segment_tts_compute_requirements_v1(
    model_group: impl Into<String>,
    min_vram_mb: u64,
) -> ComputeRequirements {
    ComputeRequirements {
        gpu: ResourceRequirement::Required,
        min_vram_mb: Some(min_vram_mb),
        model_group: Some(model_group.into()),
        parallelizable: true,
        cost_metric: Some("seconds".to_owned()),
    }
}
