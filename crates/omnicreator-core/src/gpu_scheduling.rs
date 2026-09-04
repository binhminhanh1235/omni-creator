use std::collections::BTreeSet;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::{
    ComputeProviderCapabilitiesV1, ComputeProviderConnectionState, ComputeProviderSessionV1,
    ComputeRequirements, Error, Job, LogicalUri, ResourceRequirement, Result, StateStore,
    StepStatus,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuJobPreparationV1 {
    pub job_id: String,
    pub input_resolved: bool,
    pub input_immutable: bool,
    pub plugin_id: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub model_version: Option<String>,
    pub settings_fingerprint: Option<String>,
    pub output_uri: Option<LogicalUri>,
    pub approval_required: bool,
    pub approval_complete: bool,
    pub production_lock_required: bool,
    pub preflight_required: bool,
    pub preflight_complete: bool,
    pub gpu_execution_requested: bool,
    pub requirements: ComputeRequirements,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "status")]
pub enum CacheLookupV1 {
    NotChecked,
    Miss,
    Hit { artifact_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuReadinessFactsV1 {
    pub workflow_step_status: Option<StepStatus>,
    pub dependencies_succeeded: bool,
    pub production_locked: bool,
    pub cache_lookup: CacheLookupV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeProviderSchedulingSnapshotV1 {
    pub state: ComputeProviderConnectionState,
    pub session: ComputeProviderSessionV1,
}

impl ComputeProviderSchedulingSnapshotV1 {
    pub fn validate_v1(&self) -> Result<()> {
        self.session.validate_v1()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeRunningAssignmentV1 {
    pub job_id: String,
    pub provider_id: String,
    pub session_id: String,
    pub device_id: String,
    pub parallelizable: bool,
    pub parallelism_group: String,
}

impl ComputeRunningAssignmentV1 {
    pub fn validate_v1(&self) -> Result<()> {
        require_identifier("running assignment job_id", &self.job_id)?;
        require_identifier("running assignment provider_id", &self.provider_id)?;
        require_identifier("running assignment session_id", &self.session_id)?;
        require_identifier("running assignment device_id", &self.device_id)?;
        require_identifier(
            "running assignment parallelism_group",
            &self.parallelism_group,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeDeviceSelectionV1 {
    pub provider_id: String,
    pub session_id: String,
    pub device_id: String,
    pub parallelizable: bool,
    pub parallelism_group: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GpuQueueEligibilityStatusV1 {
    GpuReady,
    NotReady,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GpuNotReadyReasonCodeV1 {
    JobStateNotSchedulable,
    WorkflowStepMissing,
    WorkflowStepNotReady,
    DependenciesIncomplete,
    InputHashMissing,
    InputNotResolved,
    InputNotImmutable,
    PluginUnknown,
    ProviderUnknown,
    ModelUnknown,
    ModelVersionUnknown,
    SettingsUnknown,
    OutputUnknown,
    CacheNotChecked,
    CacheHit,
    ApprovalPending,
    ProductionLockMissing,
    PreflightPending,
    GpuExecutionNotRequested,
    GpuNotSupported,
    ProviderUnavailable,
    ProviderNotReady,
    ModelGroupUnsupported,
    ProviderAtCapacity,
    ParallelismConflict,
    NoGpuDevice,
    InsufficientVram,
    NoAvailableGpuDevice,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuNotReadyReasonV1 {
    pub code: GpuNotReadyReasonCodeV1,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuQueueEligibilityV1 {
    pub job_id: String,
    pub status: GpuQueueEligibilityStatusV1,
    #[serde(default)]
    pub reasons: Vec<GpuNotReadyReasonV1>,
    pub selection: Option<ComputeDeviceSelectionV1>,
}

impl GpuQueueEligibilityV1 {
    pub fn is_gpu_ready(&self) -> bool {
        self.status == GpuQueueEligibilityStatusV1::GpuReady
    }
}

impl ComputeRequirements {
    pub fn validate_scheduling_v1(&self) -> Result<()> {
        if self.min_vram_mb.is_some_and(|memory| memory == 0) {
            return Err(Error::InvalidContract(
                "compute requirement min_vram_mb must be positive when present".to_owned(),
            ));
        }
        if let Some(model_group) = self.model_group.as_deref() {
            require_identifier("compute requirement model_group", model_group)?;
        }
        if let Some(cost_metric) = self.cost_metric.as_deref() {
            require_identifier("compute requirement cost_metric", cost_metric)?;
        }
        if self.gpu == ResourceRequirement::None
            && (self.min_vram_mb.is_some() || self.model_group.is_some())
        {
            return Err(Error::InvalidContract(
                "GPU-none requirements cannot declare min_vram_mb or model_group".to_owned(),
            ));
        }
        Ok(())
    }
}

impl StateStore {
    pub fn evaluate_gpu_queue(
        &self,
        preparation: &GpuJobPreparationV1,
        providers: &[ComputeProviderSchedulingSnapshotV1],
        running: &[ComputeRunningAssignmentV1],
    ) -> Result<GpuQueueEligibilityV1> {
        require_identifier("GPU preparation job_id", &preparation.job_id)?;
        let job = self.get_job(&preparation.job_id)?;
        let facts = self.gpu_readiness_facts(&job)?;

        evaluate_gpu_queue(&job, &facts, preparation, providers, running)
    }

    pub fn gpu_readiness_facts(&self, job: &Job) -> Result<GpuReadinessFactsV1> {
        let project = self.get_project(&job.project_id)?;

        let workflow_step_status = self
            .connection
            .query_row(
                "SELECT status FROM steps                  WHERE project_id=?1 AND step_key=?2 AND unit_key=?3",
                params![&job.project_id, &job.step, &job.unit],
                |row| {
                    let raw: String = row.get(0)?;
                    crate::state::parse_step_status(&raw, 0)
                },
            )
            .optional()?;

        let dependencies_succeeded = if workflow_step_status.is_some() {
            let incomplete: i64 = self.connection.query_row(
                "SELECT COUNT(*)                  FROM dependencies d                  JOIN steps downstream ON downstream.id=d.downstream_step_id                  JOIN steps upstream ON upstream.id=d.upstream_step_id                  WHERE downstream.project_id=?1                    AND downstream.step_key=?2                    AND downstream.unit_key=?3                    AND upstream.status NOT IN ('SUCCEEDED','SKIPPED')",
                params![&job.project_id, &job.step, &job.unit],
                |row| row.get(0),
            )?;
            incomplete == 0
        } else {
            false
        };

        let cache_lookup = if job.input_hash.trim().is_empty() {
            CacheLookupV1::NotChecked
        } else {
            match self.find_cached_artifact(&job.input_hash)? {
                Some(artifact) => CacheLookupV1::Hit {
                    artifact_id: artifact.artifact_id,
                },
                None => CacheLookupV1::Miss,
            }
        };

        Ok(GpuReadinessFactsV1 {
            workflow_step_status,
            dependencies_succeeded,
            production_locked: project.production_lock,
            cache_lookup,
        })
    }
}

pub fn evaluate_gpu_queue(
    job: &Job,
    facts: &GpuReadinessFactsV1,
    preparation: &GpuJobPreparationV1,
    providers: &[ComputeProviderSchedulingSnapshotV1],
    running: &[ComputeRunningAssignmentV1],
) -> Result<GpuQueueEligibilityV1> {
    preparation.requirements.validate_scheduling_v1()?;
    for provider in providers {
        provider.validate_v1()?;
    }
    for assignment in running {
        assignment.validate_v1()?;
    }

    if preparation.job_id != job.job_id {
        return Err(Error::InvalidContract(format!(
            "GPU preparation job_id {} does not match logical job {}",
            preparation.job_id, job.job_id
        )));
    }

    let mut reasons = Vec::new();

    if !matches!(job.status, StepStatus::Ready | StepStatus::Retryable) {
        push_reason(
            &mut reasons,
            GpuNotReadyReasonCodeV1::JobStateNotSchedulable,
            format!(
                "Logical job {} is {}, but GPU scheduling requires READY or RETRYABLE.",
                job.job_id,
                job.status.as_str()
            ),
        );
    }

    match facts.workflow_step_status {
        None => push_reason(
            &mut reasons,
            GpuNotReadyReasonCodeV1::WorkflowStepMissing,
            "No workflow step matches this job's project/step/unit identity.",
        ),
        Some(status) if !matches!(status, StepStatus::Ready | StepStatus::Retryable) => {
            push_reason(
                &mut reasons,
                GpuNotReadyReasonCodeV1::WorkflowStepNotReady,
                format!(
                    "Workflow step is {} and is not ready for GPU scheduling.",
                    status.as_str()
                ),
            );
        }
        Some(_) => {}
    }

    if !facts.dependencies_succeeded {
        push_reason(
            &mut reasons,
            GpuNotReadyReasonCodeV1::DependenciesIncomplete,
            "At least one upstream dependency has not succeeded or been skipped.",
        );
    }

    if job.input_hash.trim().is_empty() {
        push_reason(
            &mut reasons,
            GpuNotReadyReasonCodeV1::InputHashMissing,
            "The logical job has no immutable input hash.",
        );
    }
    if !preparation.input_resolved {
        push_reason(
            &mut reasons,
            GpuNotReadyReasonCodeV1::InputNotResolved,
            "Job inputs have not been fully resolved.",
        );
    }
    if !preparation.input_immutable {
        push_reason(
            &mut reasons,
            GpuNotReadyReasonCodeV1::InputNotImmutable,
            "Resolved job inputs are not yet immutable.",
        );
    }

    validate_optional_requirement(
        &mut reasons,
        preparation.plugin_id.as_deref(),
        GpuNotReadyReasonCodeV1::PluginUnknown,
        "Plugin is not selected.",
    );
    validate_optional_requirement(
        &mut reasons,
        preparation.provider_id.as_deref(),
        GpuNotReadyReasonCodeV1::ProviderUnknown,
        "Compute provider is not selected.",
    );
    validate_optional_requirement(
        &mut reasons,
        preparation.model_id.as_deref(),
        GpuNotReadyReasonCodeV1::ModelUnknown,
        "Model is not selected.",
    );
    validate_optional_requirement(
        &mut reasons,
        preparation.model_version.as_deref(),
        GpuNotReadyReasonCodeV1::ModelVersionUnknown,
        "Model version is not pinned.",
    );
    validate_optional_requirement(
        &mut reasons,
        preparation.settings_fingerprint.as_deref(),
        GpuNotReadyReasonCodeV1::SettingsUnknown,
        "Resolved settings fingerprint is missing.",
    );
    if preparation.output_uri.is_none() {
        push_reason(
            &mut reasons,
            GpuNotReadyReasonCodeV1::OutputUnknown,
            "Logical output destination is not resolved.",
        );
    }

    match &facts.cache_lookup {
        CacheLookupV1::NotChecked => push_reason(
            &mut reasons,
            GpuNotReadyReasonCodeV1::CacheNotChecked,
            "Cache lookup has not completed.",
        ),
        CacheLookupV1::Hit { artifact_id } => push_reason(
            &mut reasons,
            GpuNotReadyReasonCodeV1::CacheHit,
            format!("A verified local artifact already satisfies this input hash: {artifact_id}."),
        ),
        CacheLookupV1::Miss => {}
    }

    if preparation.approval_required && !preparation.approval_complete {
        push_reason(
            &mut reasons,
            GpuNotReadyReasonCodeV1::ApprovalPending,
            "Required human approval has not completed.",
        );
    }
    if preparation.production_lock_required && !facts.production_locked {
        push_reason(
            &mut reasons,
            GpuNotReadyReasonCodeV1::ProductionLockMissing,
            "The workflow requires production lock before GPU dispatch.",
        );
    }
    if preparation.preflight_required && !preparation.preflight_complete {
        push_reason(
            &mut reasons,
            GpuNotReadyReasonCodeV1::PreflightPending,
            "Required production preflight has not completed.",
        );
    }

    match preparation.requirements.gpu {
        ResourceRequirement::None => push_reason(
            &mut reasons,
            GpuNotReadyReasonCodeV1::GpuNotSupported,
            "This job declares no GPU requirement and must stay off the remote GPU queue.",
        ),
        ResourceRequirement::Optional | ResourceRequirement::Required
            if !preparation.gpu_execution_requested =>
        {
            push_reason(
                &mut reasons,
                GpuNotReadyReasonCodeV1::GpuExecutionNotRequested,
                "GPU execution was not selected for this job.",
            );
        }
        ResourceRequirement::Optional | ResourceRequirement::Required => {}
    }

    if !reasons.is_empty() {
        return Ok(not_ready(job, reasons));
    }

    let plugin_id = preparation.plugin_id.as_deref().expect("validated above");
    let provider_id = preparation.provider_id.as_deref().expect("validated above");
    let model_id = preparation.model_id.as_deref().expect("validated above");
    let parallelism_group = scheduling_group(
        plugin_id,
        model_id,
        preparation.requirements.model_group.as_deref(),
    );

    let matching_providers = providers
        .iter()
        .filter(|provider| provider.session.identity.provider_id == provider_id)
        .collect::<Vec<_>>();

    if matching_providers.is_empty() {
        return Ok(not_ready(
            job,
            vec![reason(
                GpuNotReadyReasonCodeV1::ProviderUnavailable,
                format!("Selected compute provider {provider_id} is not connected."),
            )],
        ));
    }

    let provider = matching_providers
        .iter()
        .copied()
        .filter(|provider| provider.state == ComputeProviderConnectionState::Ready)
        .min_by(|left, right| {
            left.session
                .identity
                .session_id
                .cmp(&right.session.identity.session_id)
        });

    let provider = match provider {
        Some(provider) => provider,
        None => {
            let state = matching_providers
                .iter()
                .map(|provider| provider.state.as_str())
                .min()
                .unwrap_or("UNKNOWN");
            return Ok(not_ready(
                job,
                vec![reason(
                    GpuNotReadyReasonCodeV1::ProviderNotReady,
                    format!("Selected compute provider {provider_id} has no READY session; observed {state}."),
                )],
            ));
        }
    };

    if let Some(model_group) = preparation.requirements.model_group.as_deref() {
        if !provider
            .session
            .capabilities
            .model_groups
            .iter()
            .any(|group| group == model_group)
        {
            return Ok(not_ready(
                job,
                vec![reason(
                    GpuNotReadyReasonCodeV1::ModelGroupUnsupported,
                    format!(
                        "Provider {provider_id} does not advertise required model group {model_group}."
                    ),
                )],
            ));
        }
    }

    let provider_running = running
        .iter()
        .filter(|assignment| {
            assignment.provider_id == provider_id
                && assignment.session_id == provider.session.identity.session_id
        })
        .collect::<Vec<_>>();

    if provider
        .session
        .capabilities
        .max_parallel_jobs
        .is_some_and(|limit| provider_running.len() >= limit as usize)
    {
        return Ok(not_ready(
            job,
            vec![reason(
                GpuNotReadyReasonCodeV1::ProviderAtCapacity,
                format!("Provider {provider_id} has reached max_parallel_jobs."),
            )],
        ));
    }

    if provider_running.iter().any(|assignment| {
        assignment.parallelism_group == parallelism_group
            && (!assignment.parallelizable || !preparation.requirements.parallelizable)
    }) {
        return Ok(not_ready(
            job,
            vec![reason(
                GpuNotReadyReasonCodeV1::ParallelismConflict,
                format!(
                    "Scheduling group {parallelism_group} already has a conflicting non-parallelizable job."
                ),
            )],
        ));
    }

    let all_gpu_devices = gpu_devices(&provider.session.capabilities);
    if all_gpu_devices.is_empty() {
        return Ok(not_ready(
            job,
            vec![reason(
                GpuNotReadyReasonCodeV1::NoGpuDevice,
                format!("Provider {provider_id} advertises no GPU devices."),
            )],
        ));
    }

    let min_vram = preparation.requirements.min_vram_mb.unwrap_or(0);
    let vram_compatible = all_gpu_devices
        .iter()
        .copied()
        .filter(|device| min_vram == 0 || device.memory_mb.is_some_and(|memory| memory >= min_vram))
        .collect::<Vec<_>>();

    if vram_compatible.is_empty() {
        return Ok(not_ready(
            job,
            vec![reason(
                GpuNotReadyReasonCodeV1::InsufficientVram,
                format!(
                    "No independent GPU device on provider {provider_id} satisfies minimum VRAM {min_vram} MB."
                ),
            )],
        ));
    }

    let busy_devices = provider_running
        .iter()
        .map(|assignment| assignment.device_id.as_str())
        .collect::<BTreeSet<_>>();

    let selected_device = vram_compatible
        .into_iter()
        .filter(|device| !busy_devices.contains(device.id.as_str()))
        .min_by(|left, right| left.id.cmp(&right.id));

    let device = match selected_device {
        Some(device) => device,
        None => {
            return Ok(not_ready(
                job,
                vec![reason(
                    GpuNotReadyReasonCodeV1::NoAvailableGpuDevice,
                    format!(
                        "All compatible GPU devices on provider {provider_id} are currently occupied."
                    ),
                )],
            ));
        }
    };

    Ok(GpuQueueEligibilityV1 {
        job_id: job.job_id.clone(),
        status: GpuQueueEligibilityStatusV1::GpuReady,
        reasons: Vec::new(),
        selection: Some(ComputeDeviceSelectionV1 {
            provider_id: provider_id.to_owned(),
            session_id: provider.session.identity.session_id.clone(),
            device_id: device.id.clone(),
            parallelizable: preparation.requirements.parallelizable,
            parallelism_group,
        }),
    })
}

fn gpu_devices(capabilities: &ComputeProviderCapabilitiesV1) -> Vec<&crate::ComputeDeviceV1> {
    let mut devices = capabilities
        .devices
        .iter()
        .filter(|device| device.device_type.eq_ignore_ascii_case("gpu"))
        .collect::<Vec<_>>();
    devices.sort_by(|left, right| left.id.cmp(&right.id));
    devices
}

fn scheduling_group(plugin_id: &str, model_id: &str, model_group: Option<&str>) -> String {
    model_group
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{plugin_id}::{model_id}"))
}

fn validate_optional_requirement(
    reasons: &mut Vec<GpuNotReadyReasonV1>,
    value: Option<&str>,
    code: GpuNotReadyReasonCodeV1,
    message: &str,
) {
    if match value {
        None => true,
        Some(value) => value.trim().is_empty(),
    } {
        push_reason(reasons, code, message);
    }
}

fn not_ready(job: &Job, mut reasons: Vec<GpuNotReadyReasonV1>) -> GpuQueueEligibilityV1 {
    reasons.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then_with(|| left.message.cmp(&right.message))
    });
    reasons.dedup_by(|left, right| left.code == right.code && left.message == right.message);

    GpuQueueEligibilityV1 {
        job_id: job.job_id.clone(),
        status: GpuQueueEligibilityStatusV1::NotReady,
        reasons,
        selection: None,
    }
}

fn reason(code: GpuNotReadyReasonCodeV1, message: impl Into<String>) -> GpuNotReadyReasonV1 {
    GpuNotReadyReasonV1 {
        code,
        message: message.into(),
    }
}

fn push_reason(
    reasons: &mut Vec<GpuNotReadyReasonV1>,
    code: GpuNotReadyReasonCodeV1,
    message: impl Into<String>,
) {
    reasons.push(reason(code, message));
}

fn require_identifier(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidContract(format!("{label} must not be empty")));
    }
    if value.trim() != value {
        return Err(Error::InvalidContract(format!(
            "{label} must not contain surrounding whitespace"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(Error::InvalidContract(format!(
            "{label} must not contain control characters"
        )));
    }
    Ok(())
}
