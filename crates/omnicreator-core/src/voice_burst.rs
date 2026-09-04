use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    evaluate_gpu_queue, ComputeDeviceSelectionV1, ComputeProviderSchedulingSnapshotV1,
    ComputeRunningAssignmentV1, Error, GpuNotReadyReasonCodeV1, GpuQueueEligibilityV1,
    GpuReadinessFactsV1, Job, Result, SegmentTtsPreparationV1,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct VoiceBurstAffinityKeyV1 {
    pub provider_id: Option<String>,
    pub plugin_id: Option<String>,
    pub model_group: Option<String>,
    pub model_id: String,
    pub model_version: String,
    pub voice_id: String,
    pub voice_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceBurstCandidateV1 {
    pub job: Job,
    pub readiness: GpuReadinessFactsV1,
    pub tts: SegmentTtsPreparationV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceBurstAssignmentV1 {
    pub job_id: String,
    pub project_id: String,
    pub segment_id: String,
    pub affinity: VoiceBurstAffinityKeyV1,
    pub selection: ComputeDeviceSelectionV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceBurstWaveV1 {
    pub wave_index: u32,
    #[serde(default)]
    pub assignments: Vec<VoiceBurstAssignmentV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceBurstBlockedJobV1 {
    pub job_id: String,
    pub project_id: String,
    pub segment_id: String,
    pub affinity: VoiceBurstAffinityKeyV1,
    pub decision: GpuQueueEligibilityV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceBurstPlanV1 {
    #[serde(default)]
    pub waves: Vec<VoiceBurstWaveV1>,
    #[serde(default)]
    pub blocked: Vec<VoiceBurstBlockedJobV1>,
}

impl VoiceBurstPlanV1 {
    pub fn scheduled_job_count(&self) -> usize {
        self.waves
            .iter()
            .map(|wave| wave.assignments.len())
            .sum()
    }
}

pub fn plan_voice_burst_v1(
    candidates: &[VoiceBurstCandidateV1],
    providers: &[ComputeProviderSchedulingSnapshotV1],
) -> Result<VoiceBurstPlanV1> {
    let mut prepared = Vec::with_capacity(candidates.len());
    let mut seen_job_ids = BTreeSet::new();

    for candidate in candidates {
        validate_candidate_v1(candidate)?;
        if !seen_job_ids.insert(candidate.job.job_id.clone()) {
            return Err(Error::InvalidContract(format!(
                "duplicate voice burst job_id {}",
                candidate.job.job_id
            )));
        }

        let expected_hash = candidate.tts.input_hash_v1()?;
        if candidate.job.input_hash != expected_hash {
            return Err(Error::InvalidContract(format!(
                "voice burst job {} input_hash does not match locked segment TTS input",
                candidate.job.job_id
            )));
        }

        let gpu_preparation = candidate
            .tts
            .to_gpu_job_preparation_v1(&candidate.job.job_id)?;
        prepared.push(PreparedVoiceBurstCandidateV1 {
            job: candidate.job.clone(),
            readiness: candidate.readiness.clone(),
            segment_id: candidate.tts.segment_id.clone(),
            affinity: affinity_key_v1(&candidate.tts),
            gpu_preparation,
        });
    }

    prepared.sort_by(|left, right| {
        left.affinity
            .cmp(&right.affinity)
            .then_with(|| left.job.project_id.cmp(&right.job.project_id))
            .then_with(|| left.segment_id.cmp(&right.segment_id))
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
                &candidate.gpu_preparation,
                providers,
                &running,
            )?;

            if let Some(selection) = decision.selection.clone() {
                let assignment = VoiceBurstAssignmentV1 {
                    job_id: candidate.job.job_id.clone(),
                    project_id: candidate.job.project_id.clone(),
                    segment_id: candidate.segment_id.clone(),
                    affinity: candidate.affinity.clone(),
                    selection: selection.clone(),
                };
                running.push(ComputeRunningAssignmentV1 {
                    job_id: candidate.job.job_id.clone(),
                    provider_id: selection.provider_id,
                    session_id: selection.session_id,
                    device_id: selection.device_id,
                    parallelizable: selection.parallelizable,
                    parallelism_group: selection.parallelism_group,
                });
                assignments.push(assignment);
            } else if is_wave_capacity_only(&decision) {
                deferred.push(candidate);
            } else {
                blocked.push(VoiceBurstBlockedJobV1 {
                    job_id: candidate.job.job_id,
                    project_id: candidate.job.project_id,
                    segment_id: candidate.segment_id,
                    affinity: candidate.affinity,
                    decision,
                });
            }
        }

        if assignments.is_empty() {
            for candidate in deferred {
                let decision = evaluate_gpu_queue(
                    &candidate.job,
                    &candidate.readiness,
                    &candidate.gpu_preparation,
                    providers,
                    &[],
                )?;
                blocked.push(VoiceBurstBlockedJobV1 {
                    job_id: candidate.job.job_id,
                    project_id: candidate.job.project_id,
                    segment_id: candidate.segment_id,
                    affinity: candidate.affinity,
                    decision,
                });
            }
            break;
        }

        let wave_index = u32::try_from(waves.len()).map_err(|_| {
            Error::InvalidContract("voice burst wave count exceeds u32 range".to_owned())
        })?;
        waves.push(VoiceBurstWaveV1 {
            wave_index,
            assignments,
        });
        pending = deferred;
    }

    blocked.sort_by(|left, right| {
        left.affinity
            .cmp(&right.affinity)
            .then_with(|| left.project_id.cmp(&right.project_id))
            .then_with(|| left.segment_id.cmp(&right.segment_id))
            .then_with(|| left.job_id.cmp(&right.job_id))
    });

    Ok(VoiceBurstPlanV1 { waves, blocked })
}

#[derive(Debug, Clone)]
struct PreparedVoiceBurstCandidateV1 {
    job: Job,
    readiness: GpuReadinessFactsV1,
    segment_id: String,
    affinity: VoiceBurstAffinityKeyV1,
    gpu_preparation: crate::GpuJobPreparationV1,
}

fn affinity_key_v1(tts: &SegmentTtsPreparationV1) -> VoiceBurstAffinityKeyV1 {
    VoiceBurstAffinityKeyV1 {
        provider_id: normalized_option(tts.execution.provider_id.as_deref()),
        plugin_id: normalized_option(tts.execution.plugin_id.as_deref()),
        model_group: normalized_option(tts.execution.requirements.model_group.as_deref()),
        model_id: tts.production_input.model.model_id.trim().to_owned(),
        model_version: tts.production_input.model.model_version.trim().to_owned(),
        voice_id: tts.production_input.voice.voice_id.trim().to_owned(),
        voice_version: tts.production_input.voice.voice_version.trim().to_owned(),
    }
}

fn validate_candidate_v1(candidate: &VoiceBurstCandidateV1) -> Result<()> {
    require_identifier("voice burst job_id", &candidate.job.job_id)?;
    require_identifier("voice burst project_id", &candidate.job.project_id)?;
    require_identifier("voice burst segment_id", &candidate.tts.segment_id)?;

    if candidate.job.unit != candidate.tts.segment_id {
        return Err(Error::InvalidContract(format!(
            "voice burst job {} unit {} does not match TTS segment {}",
            candidate.job.job_id, candidate.job.unit, candidate.tts.segment_id
        )));
    }
    Ok(())
}

fn is_wave_capacity_only(decision: &GpuQueueEligibilityV1) -> bool {
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

fn normalized_option(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn require_identifier(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidContract(format!("{label} must not be empty")));
    }
    Ok(())
}
