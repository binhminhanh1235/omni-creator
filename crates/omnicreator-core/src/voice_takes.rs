use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Artifact, ArtifactStore, Attempt, CacheLookupV1, Error, GpuReadinessFactsV1, Job, LogicalUri,
    Result, StateStore, StepStatus,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceTakeV1 {
    pub take_index: u32,
    pub attempt: Attempt,
    pub artifact: Option<Artifact>,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceTakeStartedV1 {
    pub take_index: u32,
    pub attempt: Attempt,
}

impl StateStore {
    pub fn request_voice_retake_v1(&self, job_id: &str) -> Result<Job> {
        let job = self.get_job(job_id)?;
        require_tts_job(&job)?;

        let existing_request: Option<String> = self
            .connection
            .query_row(
                "SELECT input_hash FROM voice_retake_requests WHERE job_id=?1",
                [job_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(input_hash) = existing_request {
            if input_hash != job.input_hash {
                return Err(Error::InvalidJobState(format!(
                    "voice retake request for job {} targets stale input",
                    job.job_id
                )));
            }
            return Ok(job);
        }

        if job.status != StepStatus::Succeeded {
            return Err(Error::InvalidJobState(format!(
                "voice retake requires a SUCCEEDED logical job; {} is {}",
                job.job_id,
                job.status.as_str()
            )));
        }
        if job.selected_attempt.is_none() || job.selected_artifact.is_none() {
            return Err(Error::InvalidJobState(
                "voice retake requires an existing selected successful take".to_owned(),
            ));
        }

        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO voice_retake_requests(job_id,input_hash,requested_at) VALUES (?1,?2,?3)",
            params![&job.job_id, &job.input_hash, Utc::now().to_rfc3339()],
        )?;
        transaction.execute(
            "UPDATE jobs SET status='READY' WHERE id=?1",
            [&job.job_id],
        )?;
        transaction.commit()?;
        self.get_job(job_id)
    }

    pub fn has_active_voice_retake_v1(&self, job_id: &str) -> Result<bool> {
        let job = self.get_job(job_id)?;
        require_tts_job(&job)?;
        let active: i64 = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM voice_retake_requests WHERE job_id=?1 AND input_hash=?2)",
            params![&job.job_id, &job.input_hash],
            |row| row.get(0),
        )?;
        Ok(active != 0)
    }

    pub fn voice_gpu_readiness_facts_v1(&self, job: &Job) -> Result<GpuReadinessFactsV1> {
        require_tts_job(job)?;
        let mut facts = self.gpu_readiness_facts(job)?;
        if self.has_active_voice_retake_v1(&job.job_id)? {
            if facts.workflow_step_status == Some(StepStatus::Succeeded) {
                facts.workflow_step_status = Some(StepStatus::Ready);
            }
            facts.cache_lookup = CacheLookupV1::Miss;
        }
        Ok(facts)
    }

    pub fn start_voice_take_attempt_v1(
        &mut self,
        job_id: &str,
        worker: Option<&str>,
    ) -> Result<VoiceTakeStartedV1> {
        let job = self.get_job(job_id)?;
        require_tts_job(&job)?;
        if !matches!(job.status, StepStatus::Ready | StepStatus::Retryable) {
            return Err(Error::InvalidJobState(format!(
                "voice take job {} cannot start from {}",
                job.job_id,
                job.status.as_str()
            )));
        }

        let attempt = Attempt {
            attempt_id: format!("attempt_{}", Uuid::new_v4().simple()),
            job_id: job.job_id.clone(),
            worker: worker.map(ToOwned::to_owned),
            started_at: Utc::now(),
            finished_at: None,
            runtime_seconds: None,
            status: StepStatus::Running,
            error_code: None,
        };

        let transaction = self.connection.transaction()?;
        let next_index: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(take_index),0)+1 FROM voice_takes WHERE job_id=?1",
            [&job.job_id],
            |row| row.get(0),
        )?;
        let take_index = u32::try_from(next_index).map_err(|_| {
            Error::InvalidContract("voice take index exceeds u32 range".to_owned())
        })?;

        transaction.execute(
            "INSERT INTO attempts(id,job_id,worker,started_at,finished_at,runtime_seconds,status,error_code) \
             VALUES (?1,?2,?3,?4,NULL,NULL,'RUNNING',NULL)",
            params![
                &attempt.attempt_id,
                &attempt.job_id,
                &attempt.worker,
                attempt.started_at.to_rfc3339(),
            ],
        )?;
        transaction.execute(
            "INSERT INTO voice_takes(attempt_id,job_id,take_index,input_hash,artifact_id,created_at) \
             VALUES (?1,?2,?3,?4,NULL,?5)",
            params![
                &attempt.attempt_id,
                &job.job_id,
                i64::from(take_index),
                &job.input_hash,
                attempt.started_at.to_rfc3339(),
            ],
        )?;
        transaction.execute(
            "UPDATE jobs SET status='RUNNING' WHERE id=?1",
            [&job.job_id],
        )?;
        transaction.commit()?;

        Ok(VoiceTakeStartedV1 {
            take_index,
            attempt,
        })
    }

    pub fn list_voice_takes_v1(&self, job_id: &str) -> Result<Vec<VoiceTakeV1>> {
        let job = self.get_job(job_id)?;
        require_tts_job(&job)?;
        let mut statement = self.connection.prepare(
            "SELECT attempt_id,take_index,artifact_id FROM voice_takes \
             WHERE job_id=?1 ORDER BY take_index,attempt_id",
        )?;
        let rows = statement
            .query_map([job_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        rows.into_iter()
            .map(|(attempt_id, take_index, artifact_id)| {
                self.voice_take_from_parts_v1(&job, attempt_id, take_index, artifact_id)
            })
            .collect()
    }

    pub fn get_voice_take_v1(&self, attempt_id: &str) -> Result<Option<VoiceTakeV1>> {
        let row = self
            .connection
            .query_row(
                "SELECT job_id,take_index,artifact_id FROM voice_takes WHERE attempt_id=?1",
                [attempt_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((job_id, take_index, artifact_id)) = row else {
            return Ok(None);
        };
        let job = self.get_job(&job_id)?;
        self.voice_take_from_parts_v1(&job, attempt_id.to_owned(), take_index, artifact_id)
            .map(Some)
    }

    pub fn select_voice_take_v1(&mut self, job_id: &str, attempt_id: &str) -> Result<Job> {
        let job = self.get_job(job_id)?;
        require_tts_job(&job)?;
        if job.status == StepStatus::Running {
            return Err(Error::InvalidJobState(
                "cannot change selected voice take while an attempt is RUNNING".to_owned(),
            ));
        }

        let take = self
            .get_voice_take_v1(attempt_id)?
            .ok_or_else(|| Error::InvalidJobState("attempt is not a voice take".to_owned()))?;
        if take.attempt.job_id != job.job_id {
            return Err(Error::InvalidJobState(
                "voice take does not belong to the requested logical job".to_owned(),
            ));
        }
        if take.attempt.status != StepStatus::Succeeded {
            return Err(Error::InvalidJobState(
                "only a SUCCEEDED voice take can be selected".to_owned(),
            ));
        }
        let artifact = take.artifact.ok_or_else(|| {
            Error::InvalidArtifact("successful voice take has no linked artifact".to_owned())
        })?;

        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE jobs SET status='SUCCEEDED',selected_attempt_id=?1,selected_artifact_id=?2 \
             WHERE id=?3",
            params![attempt_id, &artifact.artifact_id, job_id],
        )?;
        transaction.execute(
            "DELETE FROM voice_retake_requests WHERE job_id=?1",
            [job_id],
        )?;
        transaction.commit()?;
        self.get_job(job_id)
    }

    pub fn find_cached_voice_take_v1(&self, input_hash: &str) -> Result<Option<VoiceTakeV1>> {
        require_identifier("voice cache input_hash", input_hash)?;
        let row = self
            .connection
            .query_row(
                "SELECT vt.job_id,vt.attempt_id,vt.take_index,vt.artifact_id \
                 FROM voice_takes vt \
                 JOIN attempts a ON a.id=vt.attempt_id \
                 JOIN artifacts art ON art.id=vt.artifact_id \
                 WHERE vt.input_hash=?1 AND a.status='SUCCEEDED' AND art.input_hash=?1 \
                 ORDER BY art.created_at DESC,vt.take_index DESC LIMIT 1",
                [input_hash],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((job_id, attempt_id, take_index, artifact_id)) = row else {
            return Ok(None);
        };
        let job = self.get_job(&job_id)?;
        self.voice_take_from_parts_v1(&job, attempt_id, take_index, artifact_id)
            .map(Some)
    }

    fn voice_take_from_parts_v1(
        &self,
        job: &Job,
        attempt_id: String,
        take_index: i64,
        artifact_id: Option<String>,
    ) -> Result<VoiceTakeV1> {
        let take_index = u32::try_from(take_index)
            .map_err(|_| Error::InvalidContract("invalid persisted voice take index".to_owned()))?;
        let attempt = self.get_attempt(&attempt_id)?;
        let artifact = artifact_id
            .as_deref()
            .map(|id| self.get_artifact(id))
            .transpose()?;
        let selected = job.selected_attempt.as_deref() == Some(attempt_id.as_str())
            && match (&job.selected_artifact, &artifact) {
                (Some(selected_artifact), Some(artifact)) => {
                    selected_artifact == &artifact.artifact_id
                }
                _ => false,
            };
        Ok(VoiceTakeV1 {
            take_index,
            attempt,
            artifact,
            selected,
        })
    }
}

impl ArtifactStore {
    pub fn lookup_verified_voice_take_cache_v1(
        &self,
        state_store: &StateStore,
        input_hash: &str,
    ) -> Result<Option<VoiceTakeV1>> {
        let Some(take) = state_store.find_cached_voice_take_v1(input_hash)? else {
            return Ok(None);
        };
        let artifact = take.artifact.as_ref().ok_or_else(|| {
            Error::InvalidArtifact("cached voice take has no artifact".to_owned())
        })?;
        if !self.verify_artifact(artifact)? {
            return Ok(None);
        }
        Ok(Some(take))
    }
}

pub fn voice_take_output_uri_v1(base: &LogicalUri, take_index: u32) -> Result<LogicalUri> {
    if take_index == 0 {
        return Err(Error::InvalidContract(
            "voice take index must be greater than zero".to_owned(),
        ));
    }

    let (scheme, path) = match base {
        LogicalUri::Workspace(path) => ("workspace", path.as_str()),
        LogicalUri::Project(path) => ("project", path.as_str()),
        LogicalUri::Library(path) => ("library", path.as_str()),
        LogicalUri::Artifact(_) => {
            return Err(Error::InvalidContract(
                "voice take output cannot derive from artifact://".to_owned(),
            ))
        }
    };

    let base_path = Path::new(path);
    let parent = base_path.parent().unwrap_or_else(|| Path::new(""));
    let stem = base_path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| Error::InvalidContract("voice output URI has no valid file stem".to_owned()))?;
    let extension = base_path
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty());

    let mut relative = PathBuf::from(parent);
    relative.push("takes");
    relative.push(stem);
    let file_name = match extension {
        Some(extension) => format!("take-{take_index:04}.{extension}"),
        None => format!("take-{take_index:04}"),
    };
    relative.push(file_name);

    let relative = relative
        .to_str()
        .ok_or_else(|| Error::InvalidContract("voice take output URI is not UTF-8".to_owned()))?
        .replace('\\', "/");
    LogicalUri::parse(&format!("{scheme}://{relative}"))
}

pub(crate) fn attach_voice_take_artifact_transaction_v1(
    transaction: &Transaction<'_>,
    attempt_id: &str,
    artifact_id: &str,
) -> Result<bool> {
    let changed = transaction.execute(
        "UPDATE voice_takes SET artifact_id=?1 WHERE attempt_id=?2 AND artifact_id IS NULL",
        params![artifact_id, attempt_id],
    )?;
    if changed != 0 {
        return Ok(true);
    }

    let existing: Option<String> = transaction
        .query_row(
            "SELECT artifact_id FROM voice_takes WHERE attempt_id=?1",
            [attempt_id],
            |row| row.get(0),
        )
        .optional()?;
    match existing {
        Some(existing) if existing == artifact_id => Ok(true),
        Some(_) => Err(Error::InvalidArtifact(
            "voice take attempt is already linked to a different artifact".to_owned(),
        )),
        None => Ok(false),
    }
}

pub(crate) fn clear_voice_retake_request_transaction_v1(
    transaction: &Transaction<'_>,
    job_id: &str,
) -> Result<()> {
    transaction.execute(
        "DELETE FROM voice_retake_requests WHERE job_id=?1",
        [job_id],
    )?;
    Ok(())
}

fn require_tts_job(job: &Job) -> Result<()> {
    if job.step != "tts" {
        return Err(Error::InvalidJobState(format!(
            "voice take APIs require a tts job; {} uses {}",
            job.job_id, job.step
        )));
    }
    Ok(())
}

fn require_identifier(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidContract(format!("{label} must not be empty")));
    }
    Ok(())
}
