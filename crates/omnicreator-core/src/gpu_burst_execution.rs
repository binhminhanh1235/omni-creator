use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    dispatch_remote_job, remote_retry_policy_v1, ComputeProviderExecution,
    ComputeProviderSchedulingSnapshotV1, GpuBatchPlanV1, GpuBurstPlanV1, GpuQueueEligibilityV1,
    RemoteComputeJobSpecV1, RemoteDispatchStartedV1, RemoteRetryPolicyV1, Result, StateStore,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuBurstDispatchFailureV1 {
    pub job_id: String,
    pub error_code: String,
    pub message: String,
    pub retry: RemoteRetryPolicyV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuBurstDispatchSummaryV1 {
    pub burst: GpuBurstPlanV1,
    #[serde(default)]
    pub dispatched: Vec<RemoteDispatchStartedV1>,
    #[serde(default)]
    pub failures: Vec<GpuBurstDispatchFailureV1>,
}

pub fn dispatch_gpu_burst_v1(
    state_store: &mut StateStore,
    executor: &mut impl ComputeProviderExecution,
    reviewed_batch: &GpuBatchPlanV1,
    providers: &[ComputeProviderSchedulingSnapshotV1],
    specs: &[RemoteComputeJobSpecV1],
    expected_schedule_hash: &str,
) -> Result<GpuBurstDispatchSummaryV1> {
    let burst = state_store.plan_gpu_burst_v1(reviewed_batch, providers)?;
    validate_start_gate_v1(reviewed_batch, &burst, expected_schedule_hash)?;

    let specs = exact_specs_v1(reviewed_batch, specs)?;
    let jobs = reviewed_batch
        .ready_jobs
        .iter()
        .map(|job| (job.job_id.as_str(), job))
        .collect::<BTreeMap<_, _>>();

    let mut dispatched = Vec::with_capacity(burst.scheduled_job_count());
    let mut failures = Vec::new();

    for wave in &burst.waves {
        for assignment in &wave.assignments {
            let batch_job = jobs
                .get(assignment.job_id.as_str())
                .expect("burst assignments originate from reviewed ready jobs");
            let spec = specs
                .get(assignment.job_id.as_str())
                .expect("exact spec set was validated above");

            let mut eligibility: GpuQueueEligibilityV1 = batch_job.eligibility.clone();
            eligibility.selection = Some(assignment.selection.clone());

            match dispatch_remote_job(
                state_store,
                executor,
                &eligibility,
                &batch_job.preparation,
                spec,
            ) {
                Ok(started) => dispatched.push(started),
                Err(error) => {
                    let attempts = state_store.list_attempts(&assignment.job_id)?;
                    let error_code = attempts
                        .last()
                        .and_then(|attempt| attempt.error_code.clone())
                        .unwrap_or_else(|| "PROVIDER_UNAVAILABLE".to_owned());
                    failures.push(GpuBurstDispatchFailureV1 {
                        job_id: assignment.job_id.clone(),
                        retry: remote_retry_policy_v1(&error_code),
                        error_code,
                        message: error.to_string(),
                    });
                }
            }
        }
    }

    Ok(GpuBurstDispatchSummaryV1 {
        burst,
        dispatched,
        failures,
    })
}

fn validate_start_gate_v1(
    reviewed_batch: &GpuBatchPlanV1,
    burst: &GpuBurstPlanV1,
    expected_schedule_hash: &str,
) -> Result<()> {
    if expected_schedule_hash.trim().is_empty() || burst.schedule_hash != expected_schedule_hash {
        return Err(crate::Error::InvalidContract(
            "GPU Burst schedule changed after review; prepare the batch again".to_owned(),
        ));
    }
    if !reviewed_batch.is_ready_to_start()
        || !burst.blocked.is_empty()
        || !burst.preflight_blocked_job_ids.is_empty()
        || burst.scheduled_job_count() != reviewed_batch.ready_jobs.len()
    {
        return Err(crate::Error::InvalidJobState(
            "GPU Burst cannot start while reviewed work is blocked or unscheduled".to_owned(),
        ));
    }
    if burst.policy.requires_human_prompt_after_start() {
        return Err(crate::Error::InvalidContract(
            "GPU Burst execution policy must be non-interactive".to_owned(),
        ));
    }
    Ok(())
}

fn exact_specs_v1<'a>(
    reviewed_batch: &GpuBatchPlanV1,
    specs: &'a [RemoteComputeJobSpecV1],
) -> Result<BTreeMap<&'a str, &'a RemoteComputeJobSpecV1>> {
    let expected = reviewed_batch
        .ready_jobs
        .iter()
        .map(|job| job.job_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut actual = BTreeMap::new();
    for spec in specs {
        spec.validate_v1()?;
        if actual.insert(spec.job_id.as_str(), spec).is_some() {
            return Err(crate::Error::InvalidContract(format!(
                "GPU Burst contains duplicate execution spec for {}",
                spec.job_id
            )));
        }
    }
    let actual_ids = actual.keys().copied().collect::<BTreeSet<_>>();
    if expected != actual_ids {
        return Err(crate::Error::InvalidContract(
            "GPU Burst execution specs must match the reviewed ready job set exactly".to_owned(),
        ));
    }
    Ok(actual)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::{
        CacheLookupV1, ComputeDeviceV1, ComputeJobDispatchAckV1, ComputeJobDispatchV1,
        ComputeProviderCapabilitiesV1, ComputeProviderConnectionState, ComputeProviderExecution,
        ComputeProviderSessionIdentityV1, ComputeProviderSessionV1, ComputeRemoteJournalEntryV1,
        ComputeRequirements, GpuBatchPlanRequestV1, GpuJobPreparationV1, LogicalUri,
        ResourceRequirement, Workspace,
    };

    #[derive(Default)]
    struct FakeExecutor {
        dispatched: Vec<ComputeJobDispatchV1>,
    }

    impl ComputeProviderExecution for FakeExecutor {
        fn dispatch_job(
            &mut self,
            dispatch: &ComputeJobDispatchV1,
        ) -> Result<ComputeJobDispatchAckV1> {
            self.dispatched.push(dispatch.clone());
            Ok(ComputeJobDispatchAckV1 {
                job_id: dispatch.job_id.clone(),
                attempt_id: dispatch.attempt_id.clone(),
                remote_job_ref: format!("remote-{}", dispatch.job_id),
            })
        }

        fn read_journal(
            &mut self,
            _provider_id: &str,
            _session_id: &str,
            _after_sequence: Option<u64>,
        ) -> Result<Vec<ComputeRemoteJournalEntryV1>> {
            Ok(Vec::new())
        }

        fn transfer_artifact(
            &mut self,
            _provider_id: &str,
            _session_id: &str,
            _transfer_ref: &str,
            _destination: &Path,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn reviewed_burst_dispatches_every_job_in_deterministic_wave_order() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let mut store = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = store.create_project("Burst execution").unwrap();

        let first = store
            .create_job(&project.id, "tts", "S01", "hash-1")
            .unwrap();
        let second = store
            .create_job(&project.id, "tts", "S02", "hash-2")
            .unwrap();
        for job in [&first, &second] {
            store
                .set_gpu_readiness_facts(&job.job_id, true, true, CacheLookupV1::Miss)
                .unwrap();
        }

        let provider = provider();
        let preparations = vec![preparation(&first.job_id), preparation(&second.job_id)];
        let batch = store
            .plan_gpu_batch_v1(
                &GpuBatchPlanRequestV1 {
                    project_ids: vec![project.id],
                    preparations,
                },
                std::slice::from_ref(&provider),
                &[],
            )
            .unwrap();
        let burst = store
            .plan_gpu_burst_v1(&batch, std::slice::from_ref(&provider))
            .unwrap();
        assert_eq!(burst.scheduled_job_count(), 2);

        let specs = batch
            .ready_jobs
            .iter()
            .map(|job| RemoteComputeJobSpecV1 {
                job_id: job.job_id.clone(),
                operation: "tts.generate".to_owned(),
                plugin_payload: serde_json::json!({"unit": job.unit}),
            })
            .collect::<Vec<_>>();
        let mut executor = FakeExecutor::default();

        let summary = dispatch_gpu_burst_v1(
            &mut store,
            &mut executor,
            &batch,
            &[provider],
            &specs,
            &burst.schedule_hash,
        )
        .unwrap();

        assert_eq!(summary.dispatched.len(), 2);
        assert!(summary.failures.is_empty());
        assert_eq!(executor.dispatched.len(), 2);
        let expected = summary
            .burst
            .waves
            .iter()
            .flat_map(|wave| wave.assignments.iter())
            .map(|assignment| assignment.job_id.as_str())
            .collect::<Vec<_>>();
        let actual = executor
            .dispatched
            .iter()
            .map(|dispatch| dispatch.job_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        assert_ne!(
            executor.dispatched[0].device_id, executor.dispatched[1].device_id,
            "two compatible jobs should occupy independent T4 devices"
        );
    }

    fn provider() -> ComputeProviderSchedulingSnapshotV1 {
        let now = Utc.with_ymd_and_hms(2026, 9, 5, 4, 0, 0).unwrap();
        ComputeProviderSchedulingSnapshotV1 {
            state: ComputeProviderConnectionState::Ready,
            session: ComputeProviderSessionV1 {
                identity: ComputeProviderSessionIdentityV1 {
                    provider_id: "remote-gpu".to_owned(),
                    session_id: "session-p3".to_owned(),
                },
                connected_at: now,
                last_heartbeat_at: now,
                last_healthy_heartbeat_at: Some(now),
                capabilities: ComputeProviderCapabilitiesV1 {
                    schema: "omnicreator.compute-capabilities".to_owned(),
                    version: 1,
                    provider_id: "remote-gpu".to_owned(),
                    devices: vec![
                        ComputeDeviceV1 {
                            id: "gpu0".to_owned(),
                            device_type: "gpu".to_owned(),
                            model: Some("NVIDIA T4".to_owned()),
                            memory_mb: Some(15_360),
                        },
                        ComputeDeviceV1 {
                            id: "gpu1".to_owned(),
                            device_type: "gpu".to_owned(),
                            model: Some("NVIDIA T4".to_owned()),
                            memory_mb: Some(15_360),
                        },
                    ],
                    model_groups: vec!["omnivoice".to_owned()],
                    max_parallel_jobs: Some(2),
                },
            },
        }
    }

    fn preparation(job_id: &str) -> GpuJobPreparationV1 {
        GpuJobPreparationV1 {
            job_id: job_id.to_owned(),
            input_resolved: true,
            input_immutable: true,
            plugin_id: Some("omnivoice".to_owned()),
            provider_id: Some("remote-gpu".to_owned()),
            model_id: Some("omnivoice-v3".to_owned()),
            model_version: Some("3.2".to_owned()),
            settings_fingerprint: Some("voice-settings-v1".to_owned()),
            output_uri: Some(LogicalUri::parse("project://audio/output.wav").unwrap()),
            approval_required: false,
            approval_complete: true,
            production_lock_required: false,
            preflight_required: false,
            preflight_complete: true,
            gpu_execution_requested: true,
            requirements: ComputeRequirements {
                gpu: ResourceRequirement::Required,
                min_vram_mb: Some(12_000),
                model_group: Some("omnivoice".to_owned()),
                parallelizable: true,
                cost_metric: Some("seconds".to_owned()),
            },
        }
    }
}
