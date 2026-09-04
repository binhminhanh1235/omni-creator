use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use crate::{Error, Result, StateStore, StepStatus};

pub const RUNTIME_EMA_ALPHA_V1: f64 = 0.35;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeAttemptRuntimeContextV1 {
    pub attempt_id: String,
    pub provider_id: String,
    pub session_id: String,
    pub device_id: String,
    pub plugin_id: String,
    pub model_id: String,
    pub model_version: String,
    pub runtime_observation_eligible: bool,
}

impl ComputeAttemptRuntimeContextV1 {
    pub fn validate_v1(&self) -> Result<()> {
        for (label, value) in [
            ("runtime context attempt_id", self.attempt_id.as_str()),
            ("runtime context provider_id", self.provider_id.as_str()),
            ("runtime context session_id", self.session_id.as_str()),
            ("runtime context device_id", self.device_id.as_str()),
            ("runtime context plugin_id", self.plugin_id.as_str()),
            ("runtime context model_id", self.model_id.as_str()),
            ("runtime context model_version", self.model_version.as_str()),
        ] {
            require_identifier(label, value)?;
        }
        Ok(())
    }

    pub fn estimate_key(&self) -> RuntimeEstimateKeyV1 {
        RuntimeEstimateKeyV1 {
            provider_id: self.provider_id.clone(),
            device_id: self.device_id.clone(),
            plugin_id: self.plugin_id.clone(),
            model_id: self.model_id.clone(),
            model_version: self.model_version.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuntimeEstimateKeyV1 {
    pub provider_id: String,
    pub device_id: String,
    pub plugin_id: String,
    pub model_id: String,
    pub model_version: String,
}

impl RuntimeEstimateKeyV1 {
    pub fn validate_v1(&self) -> Result<()> {
        for (label, value) in [
            ("runtime estimate provider_id", self.provider_id.as_str()),
            ("runtime estimate device_id", self.device_id.as_str()),
            ("runtime estimate plugin_id", self.plugin_id.as_str()),
            ("runtime estimate model_id", self.model_id.as_str()),
            ("runtime estimate model_version", self.model_version.as_str()),
        ] {
            require_identifier(label, value)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeEstimateV1 {
    pub key: RuntimeEstimateKeyV1,
    pub sample_count: u64,
    pub total_runtime_seconds: f64,
    pub mean_runtime_seconds: f64,
    pub ema_runtime_seconds: f64,
    pub last_runtime_seconds: f64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeWorkloadItemV1 {
    pub key: RuntimeEstimateKeyV1,
    pub job_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeWorkloadEstimateLineV1 {
    pub key: RuntimeEstimateKeyV1,
    pub job_count: u64,
    pub sample_count: u64,
    pub per_job_seconds: Option<f64>,
    pub total_seconds: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeWorkloadEstimateV1 {
    pub total_jobs: u64,
    pub estimated_jobs: u64,
    pub unknown_jobs: u64,
    pub estimated_runtime_seconds: f64,
    pub lines: Vec<RuntimeWorkloadEstimateLineV1>,
}

impl StateStore {
    pub fn record_compute_attempt_runtime_context_v1(
        &self,
        context: &ComputeAttemptRuntimeContextV1,
    ) -> Result<()> {
        context.validate_v1()?;
        self.get_attempt(&context.attempt_id)?;

        self.connection.execute(
            "INSERT OR IGNORE INTO compute_attempt_contexts(             attempt_id,provider_id,session_id,device_id,plugin_id,model_id,model_version,             runtime_observation_eligible,created_at)              VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                &context.attempt_id,
                &context.provider_id,
                &context.session_id,
                &context.device_id,
                &context.plugin_id,
                &context.model_id,
                &context.model_version,
                i64::from(context.runtime_observation_eligible),
                Utc::now().to_rfc3339(),
            ],
        )?;

        let persisted = self
            .get_compute_attempt_runtime_context_v1(&context.attempt_id)?
            .ok_or_else(|| {
                Error::InvalidContract(
                    "compute attempt runtime context was not persisted".to_owned(),
                )
            })?;
        if persisted != *context {
            return Err(Error::InvalidContract(format!(
                "compute attempt {} already has a different runtime context",
                context.attempt_id
            )));
        }
        Ok(())
    }

    pub fn get_compute_attempt_runtime_context_v1(
        &self,
        attempt_id: &str,
    ) -> Result<Option<ComputeAttemptRuntimeContextV1>> {
        require_identifier("runtime context attempt_id", attempt_id)?;
        self.connection
            .query_row(
                "SELECT attempt_id,provider_id,session_id,device_id,plugin_id,model_id,model_version,                        runtime_observation_eligible                  FROM compute_attempt_contexts WHERE attempt_id=?1",
                [attempt_id],
                |row| {
                    Ok(ComputeAttemptRuntimeContextV1 {
                        attempt_id: row.get(0)?,
                        provider_id: row.get(1)?,
                        session_id: row.get(2)?,
                        device_id: row.get(3)?,
                        plugin_id: row.get(4)?,
                        model_id: row.get(5)?,
                        model_version: row.get(6)?,
                        runtime_observation_eligible: row.get::<_, i64>(7)? != 0,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn mark_compute_attempt_runtime_ineligible_v1(&self, attempt_id: &str) -> Result<bool> {
        require_identifier("runtime context attempt_id", attempt_id)?;
        let changed = self.connection.execute(
            "UPDATE compute_attempt_contexts              SET runtime_observation_eligible=0              WHERE attempt_id=?1 AND runtime_observation_eligible<>0",
            [attempt_id],
        )?;
        Ok(changed != 0)
    }

    pub fn record_runtime_observation_v1(
        &mut self,
        attempt_id: &str,
        runtime_seconds: f64,
        observed_at: DateTime<Utc>,
    ) -> Result<Option<RuntimeEstimateV1>> {
        require_identifier("runtime observation attempt_id", attempt_id)?;
        validate_runtime_seconds(runtime_seconds)?;
        let attempt = self.get_attempt(attempt_id)?;
        if attempt.status != StepStatus::Succeeded {
            return Err(Error::InvalidJobState(format!(
                "runtime observation requires SUCCEEDED attempt {}; found {}",
                attempt.attempt_id,
                attempt.status.as_str()
            )));
        }

        let transaction = self.connection.transaction()?;
        let estimate = record_runtime_observation_transaction_v1(
            &transaction,
            attempt_id,
            runtime_seconds,
            observed_at,
        )?;
        transaction.commit()?;
        Ok(estimate)
    }

    pub fn get_runtime_estimate_v1(
        &self,
        key: &RuntimeEstimateKeyV1,
    ) -> Result<Option<RuntimeEstimateV1>> {
        key.validate_v1()?;
        self.connection
            .query_row(
                "SELECT sample_count,total_runtime_seconds,mean_runtime_seconds,                        ema_runtime_seconds,last_runtime_seconds,updated_at                  FROM compute_runtime_estimates                  WHERE provider_id=?1 AND device_id=?2 AND plugin_id=?3                    AND model_id=?4 AND model_version=?5",
                params![
                    &key.provider_id,
                    &key.device_id,
                    &key.plugin_id,
                    &key.model_id,
                    &key.model_version
                ],
                |row| estimate_from_row(key.clone(), row),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_runtime_estimates_v1(&self) -> Result<Vec<RuntimeEstimateV1>> {
        let mut statement = self.connection.prepare(
            "SELECT provider_id,device_id,plugin_id,model_id,model_version,                    sample_count,total_runtime_seconds,mean_runtime_seconds,                    ema_runtime_seconds,last_runtime_seconds,updated_at              FROM compute_runtime_estimates              ORDER BY plugin_id,model_id,model_version,provider_id,device_id",
        )?;
        let rows = statement.query_map([], |row| {
            let key = RuntimeEstimateKeyV1 {
                provider_id: row.get(0)?,
                device_id: row.get(1)?,
                plugin_id: row.get(2)?,
                model_id: row.get(3)?,
                model_version: row.get(4)?,
            };
            estimate_from_row_offset(key, row, 5)
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn estimate_runtime_workload_v1(
        &self,
        items: &[RuntimeWorkloadItemV1],
    ) -> Result<RuntimeWorkloadEstimateV1> {
        let mut lines = Vec::with_capacity(items.len());
        let mut total_jobs = 0_u64;
        let mut estimated_jobs = 0_u64;
        let mut unknown_jobs = 0_u64;
        let mut estimated_runtime_seconds = 0.0_f64;

        for item in items {
            item.key.validate_v1()?;
            total_jobs = total_jobs.saturating_add(item.job_count);
            let estimate = self.get_runtime_estimate_v1(&item.key)?;
            match estimate {
                Some(estimate) => {
                    let total_seconds = estimate.ema_runtime_seconds * item.job_count as f64;
                    estimated_jobs = estimated_jobs.saturating_add(item.job_count);
                    estimated_runtime_seconds += total_seconds;
                    lines.push(RuntimeWorkloadEstimateLineV1 {
                        key: item.key.clone(),
                        job_count: item.job_count,
                        sample_count: estimate.sample_count,
                        per_job_seconds: Some(estimate.ema_runtime_seconds),
                        total_seconds: Some(total_seconds),
                    });
                }
                None => {
                    unknown_jobs = unknown_jobs.saturating_add(item.job_count);
                    lines.push(RuntimeWorkloadEstimateLineV1 {
                        key: item.key.clone(),
                        job_count: item.job_count,
                        sample_count: 0,
                        per_job_seconds: None,
                        total_seconds: None,
                    });
                }
            }
        }

        Ok(RuntimeWorkloadEstimateV1 {
            total_jobs,
            estimated_jobs,
            unknown_jobs,
            estimated_runtime_seconds,
            lines,
        })
    }
}

pub(crate) fn record_runtime_observation_transaction_v1(
    transaction: &Transaction<'_>,
    attempt_id: &str,
    runtime_seconds: f64,
    observed_at: DateTime<Utc>,
) -> Result<Option<RuntimeEstimateV1>> {
    validate_runtime_seconds(runtime_seconds)?;
    let context = transaction
        .query_row(
            "SELECT provider_id,session_id,device_id,plugin_id,model_id,model_version,                    runtime_observation_eligible              FROM compute_attempt_contexts WHERE attempt_id=?1",
            [attempt_id],
            |row| {
                Ok(ComputeAttemptRuntimeContextV1 {
                    attempt_id: attempt_id.to_owned(),
                    provider_id: row.get(0)?,
                    session_id: row.get(1)?,
                    device_id: row.get(2)?,
                    plugin_id: row.get(3)?,
                    model_id: row.get(4)?,
                    model_version: row.get(5)?,
                    runtime_observation_eligible: row.get::<_, i64>(6)? != 0,
                })
            },
        )
        .optional()?;

    let Some(context) = context else {
        return Ok(None);
    };
    if !context.runtime_observation_eligible {
        return Ok(None);
    }
    let key = context.estimate_key();

    let already_recorded: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM compute_runtime_samples WHERE attempt_id=?1)",
        [attempt_id],
        |row| row.get::<_, i64>(0).map(|value| value != 0),
    )?;
    if already_recorded {
        return runtime_estimate_from_transaction(transaction, &key);
    }

    let previous = runtime_estimate_from_transaction(transaction, &key)?;
    let next = update_estimate_v1(previous.as_ref(), key.clone(), runtime_seconds, observed_at)?;

    transaction.execute(
        "INSERT INTO compute_runtime_samples(         attempt_id,provider_id,device_id,plugin_id,model_id,model_version,runtime_seconds,observed_at)          VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            attempt_id,
            &key.provider_id,
            &key.device_id,
            &key.plugin_id,
            &key.model_id,
            &key.model_version,
            runtime_seconds,
            observed_at.to_rfc3339(),
        ],
    )?;

    transaction.execute(
        "INSERT INTO compute_runtime_estimates(         provider_id,device_id,plugin_id,model_id,model_version,sample_count,total_runtime_seconds,         mean_runtime_seconds,ema_runtime_seconds,last_runtime_seconds,updated_at)          VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)          ON CONFLICT(provider_id,device_id,plugin_id,model_id,model_version) DO UPDATE SET          sample_count=excluded.sample_count,         total_runtime_seconds=excluded.total_runtime_seconds,         mean_runtime_seconds=excluded.mean_runtime_seconds,         ema_runtime_seconds=excluded.ema_runtime_seconds,         last_runtime_seconds=excluded.last_runtime_seconds,         updated_at=excluded.updated_at",
        params![
            &next.key.provider_id,
            &next.key.device_id,
            &next.key.plugin_id,
            &next.key.model_id,
            &next.key.model_version,
            i64::try_from(next.sample_count).map_err(|_| {
                Error::InvalidContract("runtime estimate sample_count exceeds SQLite range".to_owned())
            })?,
            next.total_runtime_seconds,
            next.mean_runtime_seconds,
            next.ema_runtime_seconds,
            next.last_runtime_seconds,
            next.updated_at.to_rfc3339(),
        ],
    )?;

    Ok(Some(next))
}

pub fn update_estimate_v1(
    previous: Option<&RuntimeEstimateV1>,
    key: RuntimeEstimateKeyV1,
    runtime_seconds: f64,
    observed_at: DateTime<Utc>,
) -> Result<RuntimeEstimateV1> {
    key.validate_v1()?;
    validate_runtime_seconds(runtime_seconds)?;

    let (sample_count, total_runtime_seconds, ema_runtime_seconds) = match previous {
        Some(previous) => {
            if previous.key != key {
                return Err(Error::InvalidContract(
                    "runtime estimate key changed while applying observation".to_owned(),
                ));
            }
            let sample_count = previous.sample_count.checked_add(1).ok_or_else(|| {
                Error::InvalidContract("runtime estimate sample_count overflow".to_owned())
            })?;
            let total_runtime_seconds = previous.total_runtime_seconds + runtime_seconds;
            let ema_runtime_seconds = RUNTIME_EMA_ALPHA_V1 * runtime_seconds
                + (1.0 - RUNTIME_EMA_ALPHA_V1) * previous.ema_runtime_seconds;
            (sample_count, total_runtime_seconds, ema_runtime_seconds)
        }
        None => (1, runtime_seconds, runtime_seconds),
    };
    let mean_runtime_seconds = total_runtime_seconds / sample_count as f64;

    Ok(RuntimeEstimateV1 {
        key,
        sample_count,
        total_runtime_seconds,
        mean_runtime_seconds,
        ema_runtime_seconds,
        last_runtime_seconds: runtime_seconds,
        updated_at: observed_at,
    })
}

fn runtime_estimate_from_transaction(
    transaction: &Transaction<'_>,
    key: &RuntimeEstimateKeyV1,
) -> Result<Option<RuntimeEstimateV1>> {
    transaction
        .query_row(
            "SELECT sample_count,total_runtime_seconds,mean_runtime_seconds,                    ema_runtime_seconds,last_runtime_seconds,updated_at              FROM compute_runtime_estimates              WHERE provider_id=?1 AND device_id=?2 AND plugin_id=?3                AND model_id=?4 AND model_version=?5",
            params![
                &key.provider_id,
                &key.device_id,
                &key.plugin_id,
                &key.model_id,
                &key.model_version
            ],
            |row| estimate_from_row(key.clone(), row),
        )
        .optional()
        .map_err(Into::into)
}

fn estimate_from_row(
    key: RuntimeEstimateKeyV1,
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<RuntimeEstimateV1> {
    estimate_from_row_offset(key, row, 0)
}

fn estimate_from_row_offset(
    key: RuntimeEstimateKeyV1,
    row: &rusqlite::Row<'_>,
    offset: usize,
) -> rusqlite::Result<RuntimeEstimateV1> {
    let sample_count: i64 = row.get(offset)?;
    let updated_at: String = row.get(offset + 5)?;
    Ok(RuntimeEstimateV1 {
        key,
        sample_count: u64::try_from(sample_count).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                offset,
                rusqlite::types::Type::Integer,
                Box::new(error),
            )
        })?,
        total_runtime_seconds: row.get(offset + 1)?,
        mean_runtime_seconds: row.get(offset + 2)?,
        ema_runtime_seconds: row.get(offset + 3)?,
        last_runtime_seconds: row.get(offset + 4)?,
        updated_at: DateTime::parse_from_rfc3339(&updated_at)
            .map(|value| value.with_timezone(&Utc))
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    offset + 5,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
    })
}

fn validate_runtime_seconds(runtime_seconds: f64) -> Result<()> {
    if !runtime_seconds.is_finite() || runtime_seconds <= 0.0 {
        return Err(Error::InvalidContract(
            "runtime_seconds must be finite and greater than zero".to_owned(),
        ));
    }
    Ok(())
}

fn require_identifier(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidContract(format!("{label} must not be empty")));
    }
    Ok(())
}
