use crate::{
    dispatch_remote_job, ComputeProviderExecution, DiscoveredPlugin, Error,
    GeneratedImageExecutionDecisionV1, GeneratedImageExecutionTargetV1,
    GeneratedImagePreparationV1, GpuQueueEligibilityV1, RemoteComputeJobSpecV1,
    RemoteDispatchStartedV1, Result, StateStore, GENERATED_IMAGE_OPERATION_V1,
};

pub fn generated_image_remote_job_spec_v1(
    job_id: &str,
    preparation: &GeneratedImagePreparationV1,
) -> Result<RemoteComputeJobSpecV1> {
    if job_id.trim().is_empty() {
        return Err(Error::InvalidContract(
            "generated image remote job_id must not be empty".to_owned(),
        ));
    }
    preparation.request.validate_v1()?;

    Ok(RemoteComputeJobSpecV1 {
        job_id: job_id.to_owned(),
        operation: GENERATED_IMAGE_OPERATION_V1.to_owned(),
        plugin_payload: serde_json::to_value(&preparation.request)?,
    })
}

pub fn dispatch_generated_image_compute_provider_v1(
    state_store: &mut StateStore,
    executor: &mut impl ComputeProviderExecution,
    decision: &GeneratedImageExecutionDecisionV1,
    eligibility: &GpuQueueEligibilityV1,
    plugin: &DiscoveredPlugin,
    job_id: &str,
    preparation: &GeneratedImagePreparationV1,
) -> Result<RemoteDispatchStartedV1> {
    let target = decision.require_target_v1()?;
    if target != GeneratedImageExecutionTargetV1::ComputeProvider {
        return Err(Error::InvalidContract(
            "generated image remote dispatch requires compute_provider execution target".to_owned(),
        ));
    }
    if !preparation.gpu_execution_requested {
        return Err(Error::InvalidContract(
            "generated image remote dispatch requires gpu_execution_requested=true".to_owned(),
        ));
    }

    let preflight = preparation.preflight_v1(plugin);
    if !preflight.is_ready() {
        let codes = preflight
            .issues
            .iter()
            .map(|issue| format!("{:?}", issue.code))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Error::InvalidContract(format!(
            "generated image remote preflight blocked before dispatch: {codes}"
        )));
    }

    let gpu_preparation = preparation.to_gpu_job_preparation_v1(job_id, plugin)?;
    let spec = generated_image_remote_job_spec_v1(job_id, preparation)?;
    dispatch_remote_job(state_store, executor, eligibility, &gpu_preparation, &spec)
}
