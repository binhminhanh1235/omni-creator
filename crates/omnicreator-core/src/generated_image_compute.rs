use crate::{
    dispatch_remote_job, resolve_generated_image_execution_target_v1, ComputeProviderExecution,
    DiscoveredPlugin, Error, GeneratedImageExecutionAvailabilityV1,
    GeneratedImageExecutionPolicyV1, GeneratedImageExecutionTargetV1,
    GeneratedImagePreparationV1, RemoteComputeJobSpecV1, RemoteDispatchStartedV1, Result,
    StateStore, GENERATED_IMAGE_OPERATION_V1,
};

pub fn generated_image_remote_job_spec_v1(
    job_id: &str,
    preparation: &GeneratedImagePreparationV1,
) -> Result<RemoteComputeJobSpecV1> {
    preparation.request.validate_v1()?;
    let spec = RemoteComputeJobSpecV1 {
        job_id: job_id.to_owned(),
        operation: GENERATED_IMAGE_OPERATION_V1.to_owned(),
        plugin_payload: serde_json::to_value(&preparation.request)?,
    };
    spec.validate_v1()?;
    Ok(spec)
}

pub fn dispatch_generated_image_compute_v1(
    state_store: &mut StateStore,
    executor: &mut impl ComputeProviderExecution,
    plugin: &DiscoveredPlugin,
    job_id: &str,
    preparation: &GeneratedImagePreparationV1,
    availability: &GeneratedImageExecutionAvailabilityV1,
    policy: &GeneratedImageExecutionPolicyV1,
) -> Result<RemoteDispatchStartedV1> {
    let decision = resolve_generated_image_execution_target_v1(
        job_id,
        preparation,
        plugin,
        availability,
        policy,
    )?;
    match decision.require_target_v1()? {
        GeneratedImageExecutionTargetV1::ComputeProvider => {}
        target => {
            return Err(Error::InvalidContract(format!(
                "generated image ComputeProvider dispatch requires compute_provider target; resolved {target:?}"
            )));
        }
    }

    let eligibility = availability.compute_provider.as_ref().ok_or_else(|| {
        Error::InvalidContract(
            "generated image ComputeProvider dispatch requires canonical GPU eligibility".to_owned(),
        )
    })?;
    let gpu_preparation = preparation.to_gpu_job_preparation_v1(job_id, plugin)?;
    let spec = generated_image_remote_job_spec_v1(job_id, preparation)?;

    dispatch_remote_job(
        state_store,
        executor,
        eligibility,
        &gpu_preparation,
        &spec,
    )
}
