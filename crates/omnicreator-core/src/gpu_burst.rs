use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    deterministic_input_hash, evaluate_gpu_queue, ComputeDeviceSelectionV1,
    ComputeProviderSchedulingSnapshotV1, ComputeRunningAssignmentV1, Error, FailureDisposition,
    GpuBatchJobV1, GpuBatchPlanV1, GpuNotReadyReasonCodeV1, GpuQueueEligibilityV1, Result,
    StateStore, GPU_BATCH_PLAN_SCHEMA_V1,
};

pub const GPU_BURST_PLAN_SCHEMA_V1: &str = "omnicreator.gpu-burst-plan";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GpuBurstInteractionPolicyV1 {
    NonInteractive,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GpuBurstRetryPolicyV1 {
    ErrorAware,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GpuBurstArtifactSyncPolicyV1 {
    ImmediateVerifiedLocalCommit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuBurstExecutionPolicyV1 {
    pub interaction: GpuBurstInteractionPolicyV1,
    pub retry: GpuBurstRetryPolicyV1,
    pub artifact_sync: GpuBurstArtifactSyncPolicyV1,
}

impl GpuBurstExecutionPolicyV1 {
    pub fn default_v1() -> Self {
        Self {
            interaction: GpuBurstInteractionPolicyV1::NonInteractive,
            retry: GpuBurstRetryPolicyV1::ErrorAware,
            artifact_sync: GpuBurstArtifactSyncPolicyV1::ImmediateVerifiedLocalCommit,
        }
    }

    pub fn requires_human_prompt_after_start(&self) -> bool {
        false
    }

    pub fn should_retry_error_v1(&self, error_code: &str) -> bool {
        matches!(
            FailureDisposition::from_error_code(error_code),
            FailureDisposition::Retryable
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct GpuBurstAffinityKeyV1 {
    pub provider_id: String,
    pub plugin_id: String,
    pub model_group: String,
    pub model_id: String,
    pub model_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuBurstAssignmentV1 {
    pub job_id: String,
    pub project_id: String,
    pub step: String,
    pub unit: String,
    pub affinity: GpuBurstAffinityKeyV1,
    pub selection: ComputeDeviceSelectionV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuBurstWaveV1 {
    pub wave_index: u32,
    #[serde(default)]
    pub assignments: Vec<GpuBurstAssignmentV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuBurstBlockedJobV1 {
    pub job_id: String,
    pub project_id: String,
    pub step: String,
    pub unit: String,
    pub affinity: GpuBurstAffinityKeyV1,
    pub decision: GpuQueueEligibilityV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuBurstDeviceSummaryV1 {
    pub provider_id: String,
    pub session_id: String,
    pub device_id: String,
    pub assignment_count: u64,
    pub affinity_switches: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuBurstPlanV1 {
    pub schema: String,
    pub version: u32,
    pub batch_snapshot_hash: String,
    pub schedule_hash: String,
    pub policy: GpuBurstExecutionPolicyV1,
    #[serde(default)]
    pub waves: Vec<GpuBurstWaveV1>,
    #[serde(default)]
    pub blocked: Vec<GpuBurstBlockedJobV1>,
    #[serde(default)]
    pub preflight_blocked_job_ids: Vec<String>,
    #[serde(default)]
    pub devices: Vec<GpuBurstDeviceSummaryV1>,
}

impl GpuBurstPlanV1 {
    pub fn scheduled_job_count(&self) -> usize {
        self.waves.iter().map(|wave| wave.assignments.len()).sum()
    }

    pub fn wave_count(&self) -> usize {
        self.waves.len()
    }
}

impl StateStore {
    pub fn plan_gpu_burst_v1(
        &self,
        batch: &GpuBatchPlanV1,
        providers: &[ComputeProviderSchedulingSnapshotV1],
    ) -> Result<GpuBurstPlanV1> {
        validate_batch_v1(batch)?;

        let mut prepared = Vec::with_capacity(batch.ready_jobs.len());
        let mut seen_job_ids = BTreeSet::new();
        for batch_job in &batch.ready_jobs {
            if !seen_job_ids.insert(batch_job.job_id.clone()) {
                return Err(Error::InvalidContract(format!(
                    "GPU burst contains duplicate ready job {}",
                    batch_job.job_id
                )));
            }

            let job = self.get_job(&batch_job.job_id)?;
            validate_job_snapshot_v1(batch_job, &job)?;

            let readiness = if job.step == "tts" {
                self.voice_gpu_readiness_facts_v1(&job)?
            } else {
                self.gpu_readiness_facts(&job)?
            };

            prepared.push(PreparedGpuBurstCandidateV1 {
                job,
                readiness,
                preparation: batch_job.preparation.clone(),
                affinity: affinity_key_v1(batch_job)?,
            });
        }

        prepared.sort_by(|left, right| {
            left.affinity
                .cmp(&right.affinity)
                .then_with(|| left.job.project_id.cmp(&right.job.project_id))
                .then_with(|| left.job.step.cmp(&right.job.step))
                .then_with(|| left.job.unit.cmp(&right.job.unit))
                .then_with(|| left.job.job_id.cmp(&right.job.job_id))
        });

        let mut pending = prepared;
        let mut waves = Vec::new();
        let mut blocked = Vec::new();

        while !pending.is_empty() {
            let mut running = Vec::new();
            let mut assignments = Vec::new();
            let mut deferred = Vec::new();

            for candidate in pending {
                let decision = evaluate_gpu_queue(
                    &candidate.job,
                    &candidate.readiness,
                    &candidate.preparation,
                    providers,
                    &running,
                )?;

                if let Some(selection) = decision.selection.clone() {
                    assignments.push(GpuBurstAssignmentV1 {
                        job_id: candidate.job.job_id.clone(),
                        project_id: candidate.job.project_id.clone(),
                        step: candidate.job.step.clone(),
                        unit: candidate.job.unit.clone(),
                        affinity: candidate.affinity.clone(),
                        selection: selection.clone(),
                    });
                    running.push(ComputeRunningAssignmentV1 {
                        job_id: candidate.job.job_id.clone(),
                        provider_id: selection.provider_id,
                        session_id: selection.session_id,
                        device_id: selection.device_id,
                        parallelizable: selection.parallelizable,
                        parallelism_group: selection.parallelism_group,
                    });
                } else if is_wave_capacity_only_v1(&decision) {
                    deferred.push(candidate);
                } else {
                    blocked.push(blocked_from_candidate_v1(candidate, decision));
                }
            }

            if assignments.is_empty() {
                for candidate in deferred {
                    let decision = evaluate_gpu_queue(
                        &candidate.job,
                        &candidate.readiness,
                        &candidate.preparation,
                        providers,
                        &[],
                    )?;
                    blocked.push(blocked_from_candidate_v1(candidate, decision));
                }
                break;
            }

            assignments.sort_by(|left, right| {
                (
                    left.selection.provider_id.as_str(),
                    left.selection.session_id.as_str(),
                    left.selection.device_id.as_str(),
                    left.affinity.clone(),
                    left.project_id.as_str(),
                    left.step.as_str(),
                    left.unit.as_str(),
                    left.job_id.as_str(),
                )
                    .cmp(&(
                        right.selection.provider_id.as_str(),
                        right.selection.session_id.as_str(),
                        right.selection.device_id.as_str(),
                        right.affinity.clone(),
                        right.project_id.as_str(),
                        right.step.as_str(),
                        right.unit.as_str(),
                        right.job_id.as_str(),
                    ))
            });

            let wave_index = u32::try_from(waves.len()).map_err(|_| {
                Error::InvalidContract("GPU burst wave count exceeds u32 range".to_owned())
            })?;
            waves.push(GpuBurstWaveV1 {
                wave_index,
                assignments,
            });
            pending = deferred;
        }

        blocked.sort_by(|left, right| {
            left.affinity
                .cmp(&right.affinity)
                .then_with(|| left.project_id.cmp(&right.project_id))
                .then_with(|| left.step.cmp(&right.step))
                .then_with(|| left.unit.cmp(&right.unit))
                .then_with(|| left.job_id.cmp(&right.job_id))
        });

        let mut preflight_blocked_job_ids = batch
            .blocked_jobs
            .iter()
            .map(|job| job.job_id.clone())
            .collect::<Vec<_>>();
        preflight_blocked_job_ids.sort();
        preflight_blocked_job_ids.dedup();

        let devices = summarize_devices_v1(&waves)?;
        let policy = GpuBurstExecutionPolicyV1::default_v1();
        let schedule_hash = schedule_hash_v1(
            &batch.snapshot_hash,
            &policy,
            &waves,
            &blocked,
            &preflight_blocked_job_ids,
            &devices,
        )?;

        Ok(GpuBurstPlanV1 {
            schema: GPU_BURST_PLAN_SCHEMA_V1.to_owned(),
            version: 1,
            batch_snapshot_hash: batch.snapshot_hash.clone(),
            schedule_hash,
            policy,
            waves,
            blocked,
            preflight_blocked_job_ids,
            devices,
        })
    }
}

#[derive(Debug, Clone)]
struct PreparedGpuBurstCandidateV1 {
    job: crate::Job,
    readiness: crate::GpuReadinessFactsV1,
    preparation: crate::GpuJobPreparationV1,
    affinity: GpuBurstAffinityKeyV1,
}

fn validate_batch_v1(batch: &GpuBatchPlanV1) -> Result<()> {
    if batch.schema != GPU_BATCH_PLAN_SCHEMA_V1 || batch.version != 1 {
        return Err(Error::InvalidContract(
            "GPU burst requires gpu-batch-plan schema/version 1".to_owned(),
        ));
    }
    if batch.snapshot_hash.len() != 64
        || !batch
            .snapshot_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Error::InvalidContract(
            "GPU burst batch snapshot_hash must be a 64-character hex digest".to_owned(),
        ));
    }
    Ok(())
}

fn validate_job_snapshot_v1(batch_job: &GpuBatchJobV1, current: &crate::Job) -> Result<()> {
    if batch_job.project_id != current.project_id
        || batch_job.step != current.step
        || batch_job.unit != current.unit
        || batch_job.input_hash != current.input_hash
    {
        return Err(Error::InvalidContract(format!(
            "GPU burst job {} no longer matches the reviewed batch snapshot",
            batch_job.job_id
        )));
    }
    Ok(())
}

fn affinity_key_v1(job: &GpuBatchJobV1) -> Result<GpuBurstAffinityKeyV1> {
    let provider_id = required_value_v1(
        "provider_id",
        job.preparation.provider_id.as_deref(),
        &job.job_id,
    )?;
    let plugin_id = required_value_v1(
        "plugin_id",
        job.preparation.plugin_id.as_deref(),
        &job.job_id,
    )?;
    let model_id = required_value_v1("model_id", job.preparation.model_id.as_deref(), &job.job_id)?;
    let model_version = required_value_v1(
        "model_version",
        job.preparation.model_version.as_deref(),
        &job.job_id,
    )?;
    let model_group = job
        .preparation
        .requirements
        .model_group
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{plugin_id}::{model_id}"));

    Ok(GpuBurstAffinityKeyV1 {
        provider_id: provider_id.to_owned(),
        plugin_id: plugin_id.to_owned(),
        model_group,
        model_id: model_id.to_owned(),
        model_version: model_version.to_owned(),
    })
}

fn required_value_v1<'a>(field: &str, value: Option<&'a str>, job_id: &str) -> Result<&'a str> {
    let value = value.ok_or_else(|| {
        Error::InvalidContract(format!(
            "GPU burst ready job {job_id} is missing preparation {field}"
        ))
    })?;
    if value.trim().is_empty() {
        return Err(Error::InvalidContract(format!(
            "GPU burst ready job {job_id} has empty preparation {field}"
        )));
    }
    Ok(value)
}

fn blocked_from_candidate_v1(
    candidate: PreparedGpuBurstCandidateV1,
    decision: GpuQueueEligibilityV1,
) -> GpuBurstBlockedJobV1 {
    GpuBurstBlockedJobV1 {
        job_id: candidate.job.job_id,
        project_id: candidate.job.project_id,
        step: candidate.job.step,
        unit: candidate.job.unit,
        affinity: candidate.affinity,
        decision,
    }
}

fn is_wave_capacity_only_v1(decision: &GpuQueueEligibilityV1) -> bool {
    !decision.reasons.is_empty()
        && decision.reasons.iter().all(|reason| {
            matches!(
                reason.code,
                GpuNotReadyReasonCodeV1::ProviderAtCapacity
                    | GpuNotReadyReasonCodeV1::ParallelismConflict
                    | GpuNotReadyReasonCodeV1::NoAvailableGpuDevice
            )
        })
}

fn summarize_devices_v1(waves: &[GpuBurstWaveV1]) -> Result<Vec<GpuBurstDeviceSummaryV1>> {
    let mut summaries =
        BTreeMap::<(String, String, String), (u64, u64, GpuBurstAffinityKeyV1)>::new();

    for wave in waves {
        for assignment in &wave.assignments {
            let key = (
                assignment.selection.provider_id.clone(),
                assignment.selection.session_id.clone(),
                assignment.selection.device_id.clone(),
            );
            match summaries.get_mut(&key) {
                Some((assignment_count, affinity_switches, previous_affinity)) => {
                    *assignment_count = assignment_count.saturating_add(1);
                    if previous_affinity != &assignment.affinity {
                        *affinity_switches = affinity_switches.saturating_add(1);
                        *previous_affinity = assignment.affinity.clone();
                    }
                }
                None => {
                    summaries.insert(key, (1, 0, assignment.affinity.clone()));
                }
            }
        }
    }

    summaries
        .into_iter()
        .map(
            |((provider_id, session_id, device_id), (assignment_count, affinity_switches, _))| {
                if assignment_count == 0 {
                    return Err(Error::InvalidContract(
                        "GPU burst device summary cannot have zero assignments".to_owned(),
                    ));
                }
                Ok(GpuBurstDeviceSummaryV1 {
                    provider_id,
                    session_id,
                    device_id,
                    assignment_count,
                    affinity_switches,
                })
            },
        )
        .collect()
}

fn schedule_hash_v1(
    batch_snapshot_hash: &str,
    policy: &GpuBurstExecutionPolicyV1,
    waves: &[GpuBurstWaveV1],
    blocked: &[GpuBurstBlockedJobV1],
    preflight_blocked_job_ids: &[String],
    devices: &[GpuBurstDeviceSummaryV1],
) -> Result<String> {
    let payload = serde_json::to_vec(&(
        GPU_BURST_PLAN_SCHEMA_V1,
        1_u32,
        batch_snapshot_hash,
        policy,
        waves,
        blocked,
        preflight_blocked_job_ids,
        devices,
    ))?;
    Ok(deterministic_input_hash(&[payload.as_slice()]))
}
