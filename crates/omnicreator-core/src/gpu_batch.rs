use std::collections::{BTreeMap, BTreeSet};

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{
    deterministic_input_hash, evaluate_gpu_queue, ComputeProviderSchedulingSnapshotV1,
    ComputeRunningAssignmentV1, Error, GpuJobPreparationV1, GpuQueueEligibilityV1, Job, Result,
    StateStore,
};

pub const GPU_BATCH_PLAN_SCHEMA_V1: &str = "omnicreator.gpu-batch-plan";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuBatchPlanRequestV1 {
    pub project_ids: Vec<String>,
    pub preparations: Vec<GpuJobPreparationV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuBatchJobV1 {
    pub job_id: String,
    pub project_id: String,
    pub step: String,
    pub unit: String,
    pub input_hash: String,
    pub preparation: GpuJobPreparationV1,
    pub eligibility: GpuQueueEligibilityV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuBatchProjectSummaryV1 {
    pub project_id: String,
    pub title: String,
    pub candidate_jobs: u64,
    pub ready_jobs: u64,
    pub blocked_jobs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuBatchWorkKindSummaryV1 {
    pub step: String,
    pub plugin_id: Option<String>,
    pub candidate_jobs: u64,
    pub ready_jobs: u64,
    pub blocked_jobs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuBatchModelGroupSummaryV1 {
    pub provider_id: Option<String>,
    pub model_group: Option<String>,
    pub candidate_jobs: u64,
    pub ready_jobs: u64,
    pub blocked_jobs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GpuBatchPlanV1 {
    pub schema: String,
    pub version: u32,
    pub snapshot_hash: String,
    pub selected_project_ids: Vec<String>,
    pub candidate_jobs: u64,
    pub ready_jobs: Vec<GpuBatchJobV1>,
    pub blocked_jobs: Vec<GpuBatchJobV1>,
    pub project_summaries: Vec<GpuBatchProjectSummaryV1>,
    pub work_kind_summaries: Vec<GpuBatchWorkKindSummaryV1>,
    pub model_group_summaries: Vec<GpuBatchModelGroupSummaryV1>,
}

impl GpuBatchPlanV1 {
    pub fn is_ready_to_start(&self) -> bool {
        !self.ready_jobs.is_empty() && self.blocked_jobs.is_empty()
    }
}

impl StateStore {
    pub fn plan_gpu_batch_v1(
        &self,
        request: &GpuBatchPlanRequestV1,
        providers: &[ComputeProviderSchedulingSnapshotV1],
        running: &[ComputeRunningAssignmentV1],
    ) -> Result<GpuBatchPlanV1> {
        let selected_project_ids = normalize_project_ids_v1(&request.project_ids)?;
        let selected = selected_project_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();

        let mut project_titles = BTreeMap::new();
        for project_id in &selected_project_ids {
            let project = self.get_project(project_id)?;
            project_titles.insert(project.id, project.title);
        }

        let mut preparations = BTreeMap::new();
        for preparation in &request.preparations {
            if preparation.job_id.trim().is_empty() {
                return Err(Error::InvalidContract(
                    "GPU batch preparation job_id must not be empty".to_owned(),
                ));
            }
            if preparations
                .insert(preparation.job_id.clone(), preparation.clone())
                .is_some()
            {
                return Err(Error::InvalidContract(format!(
                    "GPU batch contains duplicate preparation for job {}",
                    preparation.job_id
                )));
            }
        }

        let mut candidates = Vec::with_capacity(preparations.len());
        for preparation in preparations.into_values() {
            let job = self.get_job(&preparation.job_id)?;
            if !selected.contains(&job.project_id) {
                return Err(Error::InvalidContract(format!(
                    "GPU batch preparation job {} belongs to unselected project {}",
                    job.job_id, job.project_id
                )));
            }

            let facts = if job.step == "tts" {
                self.voice_gpu_readiness_facts_v1(&job)?
            } else {
                self.gpu_readiness_facts(&job)?
            };
            let eligibility = evaluate_gpu_queue(&job, &facts, &preparation, providers, running)?;
            candidates.push(GpuBatchJobV1 {
                job_id: job.job_id,
                project_id: job.project_id,
                step: job.step,
                unit: job.unit,
                input_hash: job.input_hash,
                preparation,
                eligibility,
            });
        }

        candidates
            .sort_by(|left, right| batch_job_sort_key_v1(left).cmp(&batch_job_sort_key_v1(right)));

        let mut ready_jobs = Vec::new();
        let mut blocked_jobs = Vec::new();
        for candidate in candidates {
            if candidate.eligibility.is_gpu_ready() {
                ready_jobs.push(candidate);
            } else {
                blocked_jobs.push(candidate);
            }
        }

        let project_summaries = summarize_projects_v1(
            &selected_project_ids,
            &project_titles,
            &ready_jobs,
            &blocked_jobs,
        )?;
        let work_kind_summaries = summarize_work_kinds_v1(&ready_jobs, &blocked_jobs);
        let model_group_summaries = summarize_model_groups_v1(&ready_jobs, &blocked_jobs);

        let candidate_jobs = usize_to_u64_v1(ready_jobs.len().saturating_add(blocked_jobs.len()))?;
        let snapshot_hash = batch_snapshot_hash_v1(
            &selected_project_ids,
            &ready_jobs,
            &blocked_jobs,
            &project_summaries,
            &work_kind_summaries,
            &model_group_summaries,
        )?;

        Ok(GpuBatchPlanV1 {
            schema: GPU_BATCH_PLAN_SCHEMA_V1.to_owned(),
            version: 1,
            snapshot_hash,
            selected_project_ids,
            candidate_jobs,
            ready_jobs,
            blocked_jobs,
            project_summaries,
            work_kind_summaries,
            model_group_summaries,
        })
    }

    pub fn list_gpu_batch_candidate_jobs_v1(&self, project_ids: &[String]) -> Result<Vec<Job>> {
        let project_ids = normalize_project_ids_v1(project_ids)?;
        let mut jobs = Vec::new();
        for project_id in project_ids {
            self.get_project(&project_id)?;
            let mut statement = self.connection.prepare(
                "SELECT id,project_id,step_key,unit_key,status,input_hash,selected_attempt_id,selected_artifact_id \
                 FROM jobs WHERE project_id=?1 ORDER BY step_key,unit_key,id",
            )?;
            let rows = statement.query_map(params![project_id], |row| {
                let status: String = row.get(4)?;
                Ok(Job {
                    job_id: row.get(0)?,
                    project_id: row.get(1)?,
                    step: row.get(2)?,
                    unit: row.get(3)?,
                    status: crate::state::parse_step_status(&status, 4)?,
                    input_hash: row.get(5)?,
                    selected_attempt: row.get(6)?,
                    selected_artifact: row.get(7)?,
                })
            })?;
            jobs.extend(rows.collect::<std::result::Result<Vec<_>, _>>()?);
        }
        jobs.sort_by(|left, right| {
            (
                left.project_id.as_str(),
                left.step.as_str(),
                left.unit.as_str(),
                left.job_id.as_str(),
            )
                .cmp(&(
                    right.project_id.as_str(),
                    right.step.as_str(),
                    right.unit.as_str(),
                    right.job_id.as_str(),
                ))
        });
        Ok(jobs)
    }
}

fn normalize_project_ids_v1(project_ids: &[String]) -> Result<Vec<String>> {
    if project_ids.is_empty() {
        return Err(Error::InvalidContract(
            "GPU batch requires at least one selected project".to_owned(),
        ));
    }
    let mut normalized = BTreeSet::new();
    for project_id in project_ids {
        if project_id.trim().is_empty() {
            return Err(Error::InvalidContract(
                "GPU batch project_id must not be empty".to_owned(),
            ));
        }
        normalized.insert(project_id.clone());
    }
    Ok(normalized.into_iter().collect())
}

fn batch_job_sort_key_v1(job: &GpuBatchJobV1) -> (&str, &str, &str, &str) {
    (
        job.project_id.as_str(),
        job.step.as_str(),
        job.unit.as_str(),
        job.job_id.as_str(),
    )
}

fn summarize_projects_v1(
    selected_project_ids: &[String],
    titles: &BTreeMap<String, String>,
    ready_jobs: &[GpuBatchJobV1],
    blocked_jobs: &[GpuBatchJobV1],
) -> Result<Vec<GpuBatchProjectSummaryV1>> {
    let mut counts = BTreeMap::<String, (u64, u64)>::new();
    for job in ready_jobs {
        let entry = counts.entry(job.project_id.clone()).or_default();
        entry.0 = entry.0.saturating_add(1);
    }
    for job in blocked_jobs {
        let entry = counts.entry(job.project_id.clone()).or_default();
        entry.1 = entry.1.saturating_add(1);
    }

    selected_project_ids
        .iter()
        .map(|project_id| {
            let title = titles.get(project_id).ok_or_else(|| {
                Error::InvalidContract(format!("GPU batch project title missing for {project_id}"))
            })?;
            let (ready_jobs, blocked_jobs) = counts.get(project_id).copied().unwrap_or_default();
            Ok(GpuBatchProjectSummaryV1 {
                project_id: project_id.clone(),
                title: title.clone(),
                candidate_jobs: ready_jobs.saturating_add(blocked_jobs),
                ready_jobs,
                blocked_jobs,
            })
        })
        .collect()
}

fn summarize_work_kinds_v1(
    ready_jobs: &[GpuBatchJobV1],
    blocked_jobs: &[GpuBatchJobV1],
) -> Vec<GpuBatchWorkKindSummaryV1> {
    let mut counts = BTreeMap::<(String, Option<String>), (u64, u64)>::new();
    for job in ready_jobs {
        let key = (job.step.clone(), job.preparation.plugin_id.clone());
        let entry = counts.entry(key).or_default();
        entry.0 = entry.0.saturating_add(1);
    }
    for job in blocked_jobs {
        let key = (job.step.clone(), job.preparation.plugin_id.clone());
        let entry = counts.entry(key).or_default();
        entry.1 = entry.1.saturating_add(1);
    }

    counts
        .into_iter()
        .map(
            |((step, plugin_id), (ready_jobs, blocked_jobs))| GpuBatchWorkKindSummaryV1 {
                step,
                plugin_id,
                candidate_jobs: ready_jobs.saturating_add(blocked_jobs),
                ready_jobs,
                blocked_jobs,
            },
        )
        .collect()
}

fn summarize_model_groups_v1(
    ready_jobs: &[GpuBatchJobV1],
    blocked_jobs: &[GpuBatchJobV1],
) -> Vec<GpuBatchModelGroupSummaryV1> {
    let mut counts = BTreeMap::<(Option<String>, Option<String>), (u64, u64)>::new();
    for job in ready_jobs {
        let key = batch_model_group_key_v1(job);
        let entry = counts.entry(key).or_default();
        entry.0 = entry.0.saturating_add(1);
    }
    for job in blocked_jobs {
        let key = batch_model_group_key_v1(job);
        let entry = counts.entry(key).or_default();
        entry.1 = entry.1.saturating_add(1);
    }

    counts
        .into_iter()
        .map(|((provider_id, model_group), (ready_jobs, blocked_jobs))| {
            GpuBatchModelGroupSummaryV1 {
                provider_id,
                model_group,
                candidate_jobs: ready_jobs.saturating_add(blocked_jobs),
                ready_jobs,
                blocked_jobs,
            }
        })
        .collect()
}

fn batch_model_group_key_v1(job: &GpuBatchJobV1) -> (Option<String>, Option<String>) {
    (
        job.preparation.provider_id.clone(),
        job.preparation
            .requirements
            .model_group
            .clone()
            .or_else(|| job.preparation.model_id.clone()),
    )
}

fn batch_snapshot_hash_v1(
    selected_project_ids: &[String],
    ready_jobs: &[GpuBatchJobV1],
    blocked_jobs: &[GpuBatchJobV1],
    project_summaries: &[GpuBatchProjectSummaryV1],
    work_kind_summaries: &[GpuBatchWorkKindSummaryV1],
    model_group_summaries: &[GpuBatchModelGroupSummaryV1],
) -> Result<String> {
    let payload = serde_json::to_vec(&(
        GPU_BATCH_PLAN_SCHEMA_V1,
        1_u32,
        selected_project_ids,
        ready_jobs,
        blocked_jobs,
        project_summaries,
        work_kind_summaries,
        model_group_summaries,
    ))?;
    Ok(deterministic_input_hash(&[payload.as_slice()]))
}

fn usize_to_u64_v1(value: usize) -> Result<u64> {
    u64::try_from(value)
        .map_err(|_| Error::InvalidContract("GPU batch job count exceeds u64 range".to_owned()))
}
