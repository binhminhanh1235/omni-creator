use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::{
    ComputeProviderSessionV1, Error, GpuBatchPlanV1, Result, RuntimeEstimateKeyV1,
    RuntimeWorkloadEstimateV1, RuntimeWorkloadItemV1, StateStore,
};

pub const GPU_WEEKLY_BUDGET_SCHEMA_V1: &str = "omnicreator.gpu-weekly-budget";
const WEEK_SECONDS_V1: i64 = 7 * 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuWeeklyBudgetConfigV1 {
    pub provider_id: String,
    pub allowance_seconds: f64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeSessionUsageV1 {
    pub provider_id: String,
    pub session_id: String,
    pub connected_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuWeeklyBudgetStatusV1 {
    pub schema: String,
    pub version: u32,
    pub provider_id: String,
    pub week_start: DateTime<Utc>,
    pub week_end: DateTime<Utc>,
    pub allowance_seconds: f64,
    pub used_session_seconds: f64,
    pub remaining_session_seconds: f64,
    pub overage_session_seconds: f64,
    pub open_sessions: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GpuSerialBudgetSignalV1 {
    FitsKnownSerialEstimate,
    ExceedsKnownSerialEstimate,
    IndeterminateUnknownRuntime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuBatchBudgetOverviewV1 {
    pub batch_snapshot_hash: String,
    pub provider_id: String,
    pub workload: RuntimeWorkloadEstimateV1,
    pub weekly_budget: GpuWeeklyBudgetStatusV1,
    pub serial_budget_signal: GpuSerialBudgetSignalV1,
}

impl StateStore {
    pub fn set_gpu_weekly_budget_v1(
        &self,
        provider_id: &str,
        allowance_seconds: f64,
        updated_at: DateTime<Utc>,
    ) -> Result<GpuWeeklyBudgetConfigV1> {
        require_identifier_v1("GPU weekly budget provider_id", provider_id)?;
        validate_positive_seconds_v1("GPU weekly budget allowance_seconds", allowance_seconds)?;

        self.connection.execute(
            "INSERT INTO compute_weekly_budgets(provider_id,allowance_seconds,updated_at) \
             VALUES (?1,?2,?3) \
             ON CONFLICT(provider_id) DO UPDATE SET \
             allowance_seconds=excluded.allowance_seconds,updated_at=excluded.updated_at",
            params![provider_id, allowance_seconds, updated_at.to_rfc3339()],
        )?;

        Ok(GpuWeeklyBudgetConfigV1 {
            provider_id: provider_id.to_owned(),
            allowance_seconds,
            updated_at,
        })
    }

    pub fn get_gpu_weekly_budget_v1(
        &self,
        provider_id: &str,
    ) -> Result<Option<GpuWeeklyBudgetConfigV1>> {
        require_identifier_v1("GPU weekly budget provider_id", provider_id)?;
        self.connection
            .query_row(
                "SELECT allowance_seconds,updated_at FROM compute_weekly_budgets WHERE provider_id=?1",
                [provider_id],
                |row| {
                    let updated_at: String = row.get(1)?;
                    let parsed = parse_utc_v1(&updated_at, "GPU weekly budget updated_at")
                        .map_err(to_sql_conversion_v1)?;
                    Ok(GpuWeeklyBudgetConfigV1 {
                        provider_id: provider_id.to_owned(),
                        allowance_seconds: row.get(0)?,
                        updated_at: parsed,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn start_compute_session_usage_v1(
        &self,
        session: &ComputeProviderSessionV1,
    ) -> Result<ComputeSessionUsageV1> {
        session.validate_v1()?;
        let provider_id = session.identity.provider_id.as_str();
        let session_id = session.identity.session_id.as_str();

        let existing = self.get_compute_session_usage_v1(provider_id, session_id)?;
        if let Some(existing) = existing {
            if existing.connected_at != session.connected_at {
                return Err(Error::InvalidContract(format!(
                    "compute session usage {} already exists with a different connected_at",
                    session_id
                )));
            }
            return Ok(existing);
        }

        self.connection.execute(
            "INSERT INTO compute_session_usage(provider_id,session_id,connected_at,finished_at) \
             VALUES (?1,?2,?3,NULL)",
            params![provider_id, session_id, session.connected_at.to_rfc3339()],
        )?;

        Ok(ComputeSessionUsageV1 {
            provider_id: provider_id.to_owned(),
            session_id: session_id.to_owned(),
            connected_at: session.connected_at,
            finished_at: None,
        })
    }

    pub fn finish_compute_session_usage_v1(
        &self,
        provider_id: &str,
        session_id: &str,
        finished_at: DateTime<Utc>,
    ) -> Result<ComputeSessionUsageV1> {
        require_identifier_v1("compute session usage provider_id", provider_id)?;
        require_identifier_v1("compute session usage session_id", session_id)?;
        let existing = self
            .get_compute_session_usage_v1(provider_id, session_id)?
            .ok_or_else(|| {
                Error::InvalidContract(format!(
                    "compute session usage {provider_id}/{session_id} is not registered"
                ))
            })?;

        if finished_at < existing.connected_at {
            return Err(Error::InvalidContract(
                "compute session usage finished_at must not precede connected_at".to_owned(),
            ));
        }
        if let Some(previous) = existing.finished_at {
            if previous != finished_at {
                return Err(Error::InvalidContract(format!(
                    "compute session usage {provider_id}/{session_id} already has a different finished_at"
                )));
            }
            return Ok(existing);
        }

        self.connection.execute(
            "UPDATE compute_session_usage SET finished_at=?1 \
             WHERE provider_id=?2 AND session_id=?3 AND finished_at IS NULL",
            params![finished_at.to_rfc3339(), provider_id, session_id],
        )?;

        Ok(ComputeSessionUsageV1 {
            finished_at: Some(finished_at),
            ..existing
        })
    }

    pub fn get_compute_session_usage_v1(
        &self,
        provider_id: &str,
        session_id: &str,
    ) -> Result<Option<ComputeSessionUsageV1>> {
        require_identifier_v1("compute session usage provider_id", provider_id)?;
        require_identifier_v1("compute session usage session_id", session_id)?;
        self.connection
            .query_row(
                "SELECT connected_at,finished_at FROM compute_session_usage \
                 WHERE provider_id=?1 AND session_id=?2",
                params![provider_id, session_id],
                |row| {
                    let connected_at: String = row.get(0)?;
                    let finished_at: Option<String> = row.get(1)?;
                    Ok(ComputeSessionUsageV1 {
                        provider_id: provider_id.to_owned(),
                        session_id: session_id.to_owned(),
                        connected_at: parse_utc_v1(
                            &connected_at,
                            "compute session usage connected_at",
                        )
                        .map_err(to_sql_conversion_v1)?,
                        finished_at: finished_at
                            .as_deref()
                            .map(|value| {
                                parse_utc_v1(value, "compute session usage finished_at")
                                    .map_err(to_sql_conversion_v1)
                            })
                            .transpose()?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn gpu_weekly_budget_status_v1(
        &self,
        provider_id: &str,
        week_start: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Option<GpuWeeklyBudgetStatusV1>> {
        require_identifier_v1("GPU weekly budget provider_id", provider_id)?;
        if now < week_start {
            return Err(Error::InvalidContract(
                "GPU weekly budget evaluation time must not precede week_start".to_owned(),
            ));
        }
        let week_end = week_start
            .checked_add_signed(Duration::seconds(WEEK_SECONDS_V1))
            .ok_or_else(|| {
                Error::InvalidContract("GPU weekly budget week_end overflow".to_owned())
            })?;
        let Some(config) = self.get_gpu_weekly_budget_v1(provider_id)? else {
            return Ok(None);
        };
        let evaluation_end = if now < week_end { now } else { week_end };

        let mut statement = self.connection.prepare(
            "SELECT session_id,connected_at,finished_at FROM compute_session_usage \
             WHERE provider_id=?1 AND connected_at < ?2 \
               AND (finished_at IS NULL OR finished_at > ?3) \
             ORDER BY connected_at,session_id",
        )?;
        let rows = statement.query_map(
            params![
                provider_id,
                week_end.to_rfc3339(),
                week_start.to_rfc3339()
            ],
            |row| {
                let connected_at: String = row.get(1)?;
                let finished_at: Option<String> = row.get(2)?;
                Ok(ComputeSessionUsageV1 {
                    provider_id: provider_id.to_owned(),
                    session_id: row.get(0)?,
                    connected_at: parse_utc_v1(
                        &connected_at,
                        "compute session usage connected_at",
                    )
                    .map_err(to_sql_conversion_v1)?,
                    finished_at: finished_at
                        .as_deref()
                        .map(|value| {
                            parse_utc_v1(value, "compute session usage finished_at")
                                .map_err(to_sql_conversion_v1)
                        })
                        .transpose()?,
                })
            },
        )?;
        let sessions = rows.collect::<std::result::Result<Vec<_>, _>>()?;

        let mut used_session_seconds = 0.0_f64;
        let mut open_sessions = 0_u64;
        for session in sessions {
            let clipped_start = if session.connected_at < week_start {
                week_start
            } else {
                session.connected_at
            };
            let raw_end = session.finished_at.unwrap_or(evaluation_end);
            let clipped_end = if raw_end > evaluation_end {
                evaluation_end
            } else {
                raw_end
            };
            if session.finished_at.is_none()
                && session.connected_at < evaluation_end
                && evaluation_end > week_start
            {
                open_sessions = open_sessions.saturating_add(1);
            }
            if clipped_end > clipped_start {
                used_session_seconds += duration_seconds_v1(clipped_start, clipped_end)?;
            }
        }

        let remaining_session_seconds =
            (config.allowance_seconds - used_session_seconds).max(0.0);
        let overage_session_seconds =
            (used_session_seconds - config.allowance_seconds).max(0.0);

        Ok(Some(GpuWeeklyBudgetStatusV1 {
            schema: GPU_WEEKLY_BUDGET_SCHEMA_V1.to_owned(),
            version: 1,
            provider_id: provider_id.to_owned(),
            week_start,
            week_end,
            allowance_seconds: config.allowance_seconds,
            used_session_seconds,
            remaining_session_seconds,
            overage_session_seconds,
            open_sessions,
        }))
    }

    pub fn estimate_gpu_batch_workload_v1(
        &self,
        plan: &GpuBatchPlanV1,
    ) -> Result<RuntimeWorkloadEstimateV1> {
        let mut counts = BTreeMap::<RuntimeEstimateKeyV1, u64>::new();
        for job in &plan.ready_jobs {
            let selection = job.eligibility.selection.as_ref().ok_or_else(|| {
                Error::InvalidContract(format!(
                    "GPU-ready batch job {} is missing device selection",
                    job.job_id
                ))
            })?;
            let plugin_id = required_preparation_value_v1(
                "plugin_id",
                job.preparation.plugin_id.as_deref(),
                &job.job_id,
            )?;
            let model_id = required_preparation_value_v1(
                "model_id",
                job.preparation.model_id.as_deref(),
                &job.job_id,
            )?;
            let model_version = required_preparation_value_v1(
                "model_version",
                job.preparation.model_version.as_deref(),
                &job.job_id,
            )?;
            let key = RuntimeEstimateKeyV1 {
                provider_id: selection.provider_id.clone(),
                device_id: selection.device_id.clone(),
                plugin_id: plugin_id.to_owned(),
                model_id: model_id.to_owned(),
                model_version: model_version.to_owned(),
            };
            key.validate_v1()?;
            let count = counts.entry(key).or_default();
            *count = count.saturating_add(1);
        }

        let items = counts
            .into_iter()
            .map(|(key, job_count)| RuntimeWorkloadItemV1 { key, job_count })
            .collect::<Vec<_>>();
        self.estimate_runtime_workload_v1(&items)
    }

    pub fn assess_gpu_batch_budget_v1(
        &self,
        plan: &GpuBatchPlanV1,
        provider_id: &str,
        week_start: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Option<GpuBatchBudgetOverviewV1>> {
        require_identifier_v1("GPU batch budget provider_id", provider_id)?;
        for job in &plan.ready_jobs {
            let selection = job.eligibility.selection.as_ref().ok_or_else(|| {
                Error::InvalidContract(format!(
                    "GPU-ready batch job {} is missing device selection",
                    job.job_id
                ))
            })?;
            if selection.provider_id != provider_id {
                return Err(Error::InvalidContract(format!(
                    "GPU batch budget assessment for {provider_id} cannot include ready job {} assigned to {}",
                    job.job_id, selection.provider_id
                )));
            }
        }

        let workload = self.estimate_gpu_batch_workload_v1(plan)?;
        let Some(weekly_budget) =
            self.gpu_weekly_budget_status_v1(provider_id, week_start, now)?
        else {
            return Ok(None);
        };

        let serial_budget_signal = if workload.unknown_jobs != 0 {
            GpuSerialBudgetSignalV1::IndeterminateUnknownRuntime
        } else if workload.estimated_runtime_seconds > weekly_budget.remaining_session_seconds {
            GpuSerialBudgetSignalV1::ExceedsKnownSerialEstimate
        } else {
            GpuSerialBudgetSignalV1::FitsKnownSerialEstimate
        };

        Ok(Some(GpuBatchBudgetOverviewV1 {
            batch_snapshot_hash: plan.snapshot_hash.clone(),
            provider_id: provider_id.to_owned(),
            workload,
            weekly_budget,
            serial_budget_signal,
        }))
    }
}

fn required_preparation_value_v1<'a>(
    field: &str,
    value: Option<&'a str>,
    job_id: &str,
) -> Result<&'a str> {
    value.ok_or_else(|| {
        Error::InvalidContract(format!(
            "GPU-ready batch job {job_id} is missing preparation {field}"
        ))
    })
}

fn require_identifier_v1(label: &str, value: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed != value || value.chars().any(char::is_control) {
        return Err(Error::InvalidContract(format!(
            "{label} must be a non-empty identifier without surrounding whitespace or control characters"
        )));
    }
    Ok(())
}

fn validate_positive_seconds_v1(label: &str, value: f64) -> Result<()> {
    if !value.is_finite() || value <= 0.0 {
        return Err(Error::InvalidContract(format!(
            "{label} must be finite and positive"
        )));
    }
    Ok(())
}

fn parse_utc_v1(value: &str, label: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| Error::InvalidContract(format!("{label} is invalid: {error}")))
}

fn to_sql_conversion_v1(error: Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(error),
    )
}

fn duration_seconds_v1(start: DateTime<Utc>, end: DateTime<Utc>) -> Result<f64> {
    let milliseconds = end
        .signed_duration_since(start)
        .num_milliseconds();
    if milliseconds < 0 {
        return Err(Error::InvalidContract(
            "compute session usage duration must not be negative".to_owned(),
        ));
    }
    Ok(milliseconds as f64 / 1000.0)
}
