use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    dispatch_remote_voice_take_v1, evaluate_gpu_queue, plan_voice_burst_v1, remote_retry_policy_v1,
    ArtifactStore, ComputeProviderExecution, ComputeProviderSchedulingSnapshotV1,
    ComputeRequirements, CreatorContentV1, Error, GpuQueueEligibilityStatusV1, Job, LogicalUri,
    PronunciationRuleV1, RemoteComputeJobSpecV1, RemoteDispatchStartedV1, RemoteRetryPolicyV1,
    Result, SegmentTtsExecutionTargetV1, SegmentTtsLockStateV1, SegmentTtsPreparationV1,
    SegmentTtsProductionInputV1, StateStore, StepStatus, VoiceBurstCandidateV1, VoiceBurstPlanV1,
    VoiceIdentityV1, VoiceModelIdentityV1, WorkflowStep, CREATOR_STEP_CONTENT_PREPARE_V1,
    CREATOR_STEP_PRODUCTION_PACK_V1, CREATOR_STEP_VOICE_PREPARE_V1,
    CREATOR_WORKFLOW_UNIT_PROJECT_V1,
};

pub const CREATOR_TTS_STEP_V1: &str = "tts";
pub const CREATOR_VOICE_OPERATION_V1: &str = "tts.generate";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CreatorVoiceRuntimeV1 {
    pub plugin_id: String,
    pub provider_id: String,
    pub voice: VoiceIdentityV1,
    pub model: VoiceModelIdentityV1,
    pub settings_fingerprint: String,
    #[serde(default)]
    pub pronunciation_rules: Vec<PronunciationRuleV1>,
    pub locks: SegmentTtsLockStateV1,
    pub approval_required: bool,
    pub approval_complete: bool,
    pub production_lock_required: bool,
    pub gpu_execution_requested: bool,
    pub requirements: ComputeRequirements,
}

impl CreatorVoiceRuntimeV1 {
    pub fn validate_v1(&self) -> Result<()> {
        for (label, value) in [
            ("creator voice plugin_id", self.plugin_id.as_str()),
            ("creator voice provider_id", self.provider_id.as_str()),
            ("creator voice voice_id", self.voice.voice_id.as_str()),
            (
                "creator voice voice_version",
                self.voice.voice_version.as_str(),
            ),
            ("creator voice model_id", self.model.model_id.as_str()),
            (
                "creator voice model_version",
                self.model.model_version.as_str(),
            ),
            (
                "creator voice settings_fingerprint",
                self.settings_fingerprint.as_str(),
            ),
        ] {
            require_identifier_v1(label, value)?;
        }
        if !self.locks.input_immutable() {
            return Err(Error::InvalidContract(
                "creator voice normalization and pronunciation must be locked before orchestration"
                    .to_owned(),
            ));
        }
        if !self.gpu_execution_requested {
            return Err(Error::InvalidContract(
                "creator voice P3 requires GPU execution through ComputeProvider".to_owned(),
            ));
        }
        self.requirements.validate_scheduling_v1()?;
        Ok(())
    }

    fn preparation_v1(&self, segment: &crate::SegmentV1) -> Result<SegmentTtsPreparationV1> {
        self.validate_v1()?;
        segment.validate_v1()?;
        let preparation = SegmentTtsPreparationV1 {
            segment_id: segment.id.clone(),
            production_input: SegmentTtsProductionInputV1::from_segment_v1(
                segment,
                self.pronunciation_rules.clone(),
                self.voice.clone(),
                self.model.clone(),
                self.settings_fingerprint.clone(),
            ),
            locks: self.locks.clone(),
            execution: SegmentTtsExecutionTargetV1 {
                plugin_id: Some(self.plugin_id.clone()),
                provider_id: Some(self.provider_id.clone()),
                output_uri: Some(LogicalUri::parse(&format!(
                    "project://audio/{}.wav",
                    segment.id
                ))?),
                approval_required: self.approval_required,
                approval_complete: self.approval_complete,
                production_lock_required: self.production_lock_required,
                gpu_execution_requested: self.gpu_execution_requested,
                requirements: self.requirements.clone(),
            },
        };
        preparation.preflight_v1()?;
        Ok(preparation)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreatorVoiceSegmentPlanV1 {
    pub segment_id: String,
    pub step: WorkflowStep,
    pub job: Job,
    pub tts: SegmentTtsPreparationV1,
    pub remote_spec: RemoteComputeJobSpecV1,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreatorVoiceOrchestrationPlanV1 {
    pub project_id: String,
    pub voice_step_id: String,
    #[serde(default)]
    pub segments: Vec<CreatorVoiceSegmentPlanV1>,
    pub burst: VoiceBurstPlanV1,
    #[serde(default)]
    pub completed_segment_ids: Vec<String>,
    #[serde(default)]
    pub in_flight_job_ids: Vec<String>,
}

impl CreatorVoiceOrchestrationPlanV1 {
    pub fn all_complete(&self) -> bool {
        !self.segments.is_empty() && self.completed_segment_ids.len() == self.segments.len()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreatorVoiceDispatchFailureV1 {
    pub job_id: String,
    pub error_code: String,
    pub message: String,
    pub retry: RemoteRetryPolicyV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreatorVoiceDispatchSummaryV1 {
    #[serde(default)]
    pub dispatched: Vec<RemoteDispatchStartedV1>,
    #[serde(default)]
    pub failures: Vec<CreatorVoiceDispatchFailureV1>,
}

pub fn plan_creator_voice_orchestration_v1(
    state_store: &mut StateStore,
    artifact_store: &ArtifactStore,
    content: &CreatorContentV1,
    runtime: &CreatorVoiceRuntimeV1,
    providers: &[ComputeProviderSchedulingSnapshotV1],
) -> Result<CreatorVoiceOrchestrationPlanV1> {
    content.validate_v1()?;
    runtime.validate_v1()?;
    let project = state_store.get_project(&content.project_id)?;
    if !matches!(project.studio_pack.as_deref(), Some(value) if !value.is_empty()) {
        return Err(Error::InvalidContract(
            "creator voice orchestration requires a Project bound to a Studio Pack".to_owned(),
        ));
    }
    for provider in providers {
        provider.validate_v1()?;
    }

    let (content_step, mut voice_step) =
        require_creator_voice_parent_steps_v1(state_store, &project.id)?;
    if content_step.status != StepStatus::Succeeded {
        return Err(Error::InvalidJobState(format!(
            "{} must be SUCCEEDED before creator voice orchestration; found {}",
            CREATOR_STEP_CONTENT_PREPARE_V1,
            content_step.status.as_str()
        )));
    }

    let mut existing_steps = state_store
        .list_project_steps(&project.id)?
        .into_iter()
        .map(|step| ((step.step.clone(), step.unit.clone()), step))
        .collect::<BTreeMap<_, _>>();
    let mut existing_jobs = state_store.list_project_jobs(&project.id)?;

    let mut segments = Vec::with_capacity(content.segments.len());
    for segment in &content.segments {
        let tts = runtime.preparation_v1(segment)?;
        let input_hash = tts.input_hash_v1()?;
        let mut step = ensure_segment_step_v1(
            state_store,
            &mut existing_steps,
            &content_step,
            &voice_step,
            &project.id,
            &segment.id,
            &input_hash,
        )?;

        let mut job = match find_matching_job_v1(&existing_jobs, &segment.id, &input_hash) {
            Some(job) => job,
            None => state_store.create_job(
                &project.id,
                CREATOR_TTS_STEP_V1,
                &segment.id,
                &input_hash,
            )?,
        };

        let mut completed = verified_voice_job_complete_v1(state_store, artifact_store, &job)?;
        if job.status == StepStatus::Succeeded && !completed {
            state_store.invalidate_from(&step.step_id, None)?;
            normalize_stale_step_to_not_ready_v1(state_store, &step.step_id)?;
            state_store.refresh_ready_steps(&project.id)?;
            step = state_store.get_step(&step.step_id)?;
            existing_jobs = state_store.list_project_jobs(&project.id)?;
            job = state_store.create_job(
                &project.id,
                CREATOR_TTS_STEP_V1,
                &segment.id,
                &input_hash,
            )?;
            existing_jobs.push(job.clone());
            completed = false;
        } else if !existing_jobs
            .iter()
            .any(|candidate| candidate.job_id == job.job_id)
        {
            existing_jobs.push(job.clone());
        }

        let remote_spec = RemoteComputeJobSpecV1 {
            job_id: job.job_id.clone(),
            operation: CREATOR_VOICE_OPERATION_V1.to_owned(),
            plugin_payload: serde_json::json!({
                "segment_id": &segment.id,
                "tts": &tts.production_input,
            }),
        };
        remote_spec.validate_v1()?;

        segments.push(CreatorVoiceSegmentPlanV1 {
            segment_id: segment.id.clone(),
            step,
            job,
            tts,
            remote_spec,
            completed,
        });
    }

    let all_complete = segments.iter().all(|segment| segment.completed);
    if !all_complete {
        if voice_step.status != StepStatus::NotReady {
            state_store.invalidate_from(&voice_step.step_id, None)?;
        }
        normalize_stale_step_to_not_ready_v1(state_store, &voice_step.step_id)?;
    } else if voice_step.status == StepStatus::Stale {
        state_store.set_step_status(&voice_step.step_id, StepStatus::NotReady)?;
    }

    state_store.refresh_ready_steps(&project.id)?;

    for segment in &mut segments {
        segment.step = state_store.get_step(&segment.step.step_id)?;
        segment.job = state_store.get_job(&segment.job.job_id)?;
        segment.completed =
            verified_voice_job_complete_v1(state_store, artifact_store, &segment.job)?;
        if segment.completed && segment.step.status != StepStatus::Succeeded {
            if segment.step.status == StepStatus::Stale {
                state_store.set_step_status(&segment.step.step_id, StepStatus::Ready)?;
            }
            let current = state_store.get_step(&segment.step.step_id)?;
            if current.status == StepStatus::NotReady {
                state_store.refresh_ready_steps(&project.id)?;
            }
            let current = state_store.get_step(&segment.step.step_id)?;
            if current.status == StepStatus::Ready || current.status == StepStatus::Retryable {
                state_store.set_step_status(&segment.step.step_id, StepStatus::Succeeded)?;
            }
            segment.step = state_store.get_step(&segment.step.step_id)?;
        }
    }

    state_store.refresh_ready_steps(&project.id)?;
    voice_step = state_store.get_step(&voice_step.step_id)?;
    let all_complete = segments.iter().all(|segment| segment.completed);
    if all_complete {
        if voice_step.status == StepStatus::Stale {
            state_store.set_step_status(&voice_step.step_id, StepStatus::NotReady)?;
            state_store.refresh_ready_steps(&project.id)?;
            voice_step = state_store.get_step(&voice_step.step_id)?;
        }
        if voice_step.status == StepStatus::Ready {
            state_store.set_step_status(&voice_step.step_id, StepStatus::Succeeded)?;
            state_store.refresh_ready_steps(&project.id)?;
            voice_step = state_store.get_step(&voice_step.step_id)?;
        }
    }

    let mut candidates = Vec::new();
    let mut completed_segment_ids = Vec::new();
    let mut in_flight_job_ids = Vec::new();
    for segment in &mut segments {
        segment.step = state_store.get_step(&segment.step.step_id)?;
        segment.job = state_store.get_job(&segment.job.job_id)?;
        if segment.completed {
            completed_segment_ids.push(segment.segment_id.clone());
            continue;
        }
        if segment.job.status == StepStatus::Running {
            in_flight_job_ids.push(segment.job.job_id.clone());
            continue;
        }

        let readiness = state_store.voice_gpu_readiness_facts_v1(&segment.job)?;
        let preparation = segment.tts.to_gpu_job_preparation_v1(&segment.job.job_id)?;
        let decision = evaluate_gpu_queue(&segment.job, &readiness, &preparation, providers, &[])?;
        if decision.status == GpuQueueEligibilityStatusV1::GpuReady || !decision.reasons.is_empty()
        {
            candidates.push(VoiceBurstCandidateV1 {
                job: segment.job.clone(),
                readiness,
                tts: segment.tts.clone(),
            });
        }
    }

    completed_segment_ids.sort();
    in_flight_job_ids.sort();
    let burst = plan_voice_burst_v1(&candidates, providers)?;

    Ok(CreatorVoiceOrchestrationPlanV1 {
        project_id: project.id,
        voice_step_id: voice_step.step_id,
        segments,
        burst,
        completed_segment_ids,
        in_flight_job_ids,
    })
}

pub fn dispatch_creator_voice_burst_v1(
    state_store: &mut StateStore,
    executor: &mut impl ComputeProviderExecution,
    plan: &CreatorVoiceOrchestrationPlanV1,
) -> Result<CreatorVoiceDispatchSummaryV1> {
    require_identifier_v1("creator voice project_id", &plan.project_id)?;
    require_identifier_v1("creator voice voice_step_id", &plan.voice_step_id)?;

    let by_job = plan
        .segments
        .iter()
        .map(|segment| (segment.job.job_id.as_str(), segment))
        .collect::<BTreeMap<_, _>>();

    let mut dispatched = Vec::new();
    let mut failures = Vec::new();

    for wave in &plan.burst.waves {
        for assignment in &wave.assignments {
            let segment = by_job.get(assignment.job_id.as_str()).ok_or_else(|| {
                Error::InvalidContract(format!(
                    "creator voice burst references unknown job {}",
                    assignment.job_id
                ))
            })?;
            if segment.completed {
                return Err(Error::InvalidContract(format!(
                    "creator voice burst must not dispatch completed job {}",
                    assignment.job_id
                )));
            }

            let preparation = segment.tts.to_gpu_job_preparation_v1(&segment.job.job_id)?;
            let eligibility = crate::GpuQueueEligibilityV1 {
                job_id: segment.job.job_id.clone(),
                status: GpuQueueEligibilityStatusV1::GpuReady,
                reasons: Vec::new(),
                selection: Some(assignment.selection.clone()),
            };

            match dispatch_remote_voice_take_v1(
                state_store,
                executor,
                &eligibility,
                &preparation,
                &segment.remote_spec,
            ) {
                Ok(started) => dispatched.push(started),
                Err(error) => {
                    let attempts = state_store.list_attempts(&segment.job.job_id)?;
                    let error_code = attempts
                        .last()
                        .and_then(|attempt| attempt.error_code.clone())
                        .unwrap_or_else(|| "PROVIDER_UNAVAILABLE".to_owned());
                    failures.push(CreatorVoiceDispatchFailureV1 {
                        job_id: segment.job.job_id.clone(),
                        retry: remote_retry_policy_v1(&error_code),
                        error_code,
                        message: error.to_string(),
                    });
                }
            }
        }
    }

    Ok(CreatorVoiceDispatchSummaryV1 {
        dispatched,
        failures,
    })
}

fn require_creator_voice_parent_steps_v1(
    state_store: &StateStore,
    project_id: &str,
) -> Result<(WorkflowStep, WorkflowStep)> {
    let steps = state_store.list_project_steps(project_id)?;
    let content = steps
        .iter()
        .find(|step| {
            step.step == CREATOR_STEP_CONTENT_PREPARE_V1
                && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1
        })
        .cloned()
        .ok_or_else(|| {
            Error::InvalidJobState(
                "creator voice orchestration requires the Phase 15 content.prepare step".to_owned(),
            )
        })?;
    let voice = steps
        .iter()
        .find(|step| {
            step.step == CREATOR_STEP_VOICE_PREPARE_V1
                && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1
        })
        .cloned()
        .ok_or_else(|| {
            Error::InvalidJobState(
                "creator voice orchestration requires the Phase 15 voice.prepare step".to_owned(),
            )
        })?;

    if !steps.iter().any(|step| {
        step.step == CREATOR_STEP_PRODUCTION_PACK_V1
            && step.unit == CREATOR_WORKFLOW_UNIT_PROJECT_V1
    }) {
        return Err(Error::InvalidJobState(
            "creator voice orchestration requires the Phase 15 production.pack step".to_owned(),
        ));
    }
    Ok((content, voice))
}

fn ensure_segment_step_v1(
    state_store: &mut StateStore,
    existing: &mut BTreeMap<(String, String), WorkflowStep>,
    content_step: &WorkflowStep,
    voice_step: &WorkflowStep,
    project_id: &str,
    segment_id: &str,
    input_hash: &str,
) -> Result<WorkflowStep> {
    let identity = (CREATOR_TTS_STEP_V1.to_owned(), segment_id.to_owned());
    let mut step = if let Some(step) = existing.get(&identity).cloned() {
        if step.input_hash.as_deref() != Some(input_hash) {
            state_store.invalidate_from(&step.step_id, Some(input_hash))?;
            normalize_stale_step_to_not_ready_v1(state_store, &step.step_id)?;
            state_store.get_step(&step.step_id)?
        } else {
            step
        }
    } else {
        let step = state_store.create_step(
            project_id,
            CREATOR_TTS_STEP_V1,
            segment_id,
            StepStatus::NotReady,
            Some(input_hash),
        )?;
        existing.insert(identity, step.clone());
        step
    };

    state_store.add_dependency(&content_step.step_id, &step.step_id)?;
    state_store.add_dependency(&step.step_id, &voice_step.step_id)?;
    if step.status == StepStatus::Stale {
        state_store.set_step_status(&step.step_id, StepStatus::NotReady)?;
        step = state_store.get_step(&step.step_id)?;
    }
    Ok(step)
}

fn normalize_stale_step_to_not_ready_v1(state_store: &StateStore, step_id: &str) -> Result<()> {
    let step = state_store.get_step(step_id)?;
    if step.status == StepStatus::Stale {
        state_store.set_step_status(step_id, StepStatus::NotReady)?;
    }
    Ok(())
}

fn find_matching_job_v1(jobs: &[Job], segment_id: &str, input_hash: &str) -> Option<Job> {
    jobs.iter()
        .filter(|job| {
            job.step == CREATOR_TTS_STEP_V1
                && job.unit == segment_id
                && job.input_hash == input_hash
                && !matches!(job.status, StepStatus::Stale | StepStatus::Cancelled)
        })
        .max_by_key(|job| (job_status_priority_v1(job.status), job.job_id.as_str()))
        .cloned()
}

fn job_status_priority_v1(status: StepStatus) -> u8 {
    match status {
        StepStatus::Succeeded => 6,
        StepStatus::Running => 5,
        StepStatus::Retryable => 4,
        StepStatus::Ready | StepStatus::Queued => 3,
        StepStatus::Failed | StepStatus::Fatal => 2,
        StepStatus::NotReady | StepStatus::Skipped => 1,
        StepStatus::Stale | StepStatus::Cancelled => 0,
    }
}

fn verified_voice_job_complete_v1(
    state_store: &StateStore,
    artifact_store: &ArtifactStore,
    job: &Job,
) -> Result<bool> {
    if job.status != StepStatus::Succeeded {
        return Ok(false);
    }
    let Some(attempt_id) = job.selected_attempt.as_deref() else {
        return Ok(false);
    };
    let Some(take) = state_store.get_voice_take_v1(attempt_id)? else {
        return Ok(false);
    };
    if !take.selected {
        return Ok(false);
    }
    let Some(audio) = take.artifact.as_ref() else {
        return Ok(false);
    };
    let Some(timing) = take.timing_artifact.as_ref() else {
        return Ok(false);
    };
    Ok(artifact_store.verify_artifact(audio)? && artifact_store.verify_artifact(timing)?)
}

fn require_identifier_v1(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidContract(format!("{label} must not be empty")));
    }
    Ok(())
}
