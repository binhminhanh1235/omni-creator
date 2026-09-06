use serde::Serialize;

use crate::{Job, ProjectDisplayStatus, StepStatus, WorkflowStep};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectBoardColumnV1 {
    Ideas,
    Preparing,
    NeedsReview,
    GpuReady,
    GpuRunning,
    ReadyToEdit,
    Done,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProjectBoardProjectionV1 {
    pub column: ProjectBoardColumnV1,
    pub summary: String,
    pub actionable_count: usize,
}

pub fn project_board_projection_v1(
    status: ProjectDisplayStatus,
    jobs: &[Job],
    steps: &[WorkflowStep],
) -> ProjectBoardProjectionV1 {
    let column = match status {
        ProjectDisplayStatus::Draft => ProjectBoardColumnV1::Ideas,
        ProjectDisplayStatus::Preparing => ProjectBoardColumnV1::Preparing,
        ProjectDisplayStatus::NeedsReview | ProjectDisplayStatus::GpuPartial => {
            ProjectBoardColumnV1::NeedsReview
        }
        ProjectDisplayStatus::GpuReady => ProjectBoardColumnV1::GpuReady,
        ProjectDisplayStatus::GpuRunning => ProjectBoardColumnV1::GpuRunning,
        ProjectDisplayStatus::ReadyForEdit => ProjectBoardColumnV1::ReadyToEdit,
        ProjectDisplayStatus::Done => ProjectBoardColumnV1::Done,
    };

    let retryable = jobs
        .iter()
        .filter(|job| job.status == StepStatus::Retryable)
        .count();
    let failed = jobs
        .iter()
        .filter(|job| matches!(job.status, StepStatus::Failed | StepStatus::Fatal))
        .count();
    let running = jobs
        .iter()
        .filter(|job| job.status == StepStatus::Running)
        .count();
    let gpu_ready = jobs
        .iter()
        .filter(|job| matches!(job.status, StepStatus::Ready | StepStatus::Queued))
        .count();
    let unfinished_steps = steps
        .iter()
        .filter(|step| !matches!(step.status, StepStatus::Succeeded | StepStatus::Skipped))
        .count();

    let (summary, actionable_count) = match column {
        ProjectBoardColumnV1::Ideas => (
            "Choose a Studio Pack and start preparation.".to_owned(),
            1,
        ),
        ProjectBoardColumnV1::Preparing => {
            let count = unfinished_steps.max(1);
            (
                format!("{count} preparation step{} remaining.", plural(count)),
                count,
            )
        }
        ProjectBoardColumnV1::NeedsReview => {
            if retryable > 0 {
                (
                    format!("{retryable} retryable GPU job{}.", plural(retryable)),
                    retryable,
                )
            } else if failed > 0 {
                (
                    format!("{failed} job{} need review.", plural(failed)),
                    failed,
                )
            } else {
                ("Review blocking workflow state.".to_owned(), 1)
            }
        }
        ProjectBoardColumnV1::GpuReady => {
            let count = gpu_ready.max(1);
            (
                format!("{count} GPU job{} ready.", plural(count)),
                count,
            )
        }
        ProjectBoardColumnV1::GpuRunning => {
            let count = running.max(1);
            (
                format!("{count} GPU job{} running.", plural(count)),
                count,
            )
        }
        ProjectBoardColumnV1::ReadyToEdit => (
            "Open the Production Pack and continue creative editing.".to_owned(),
            1,
        ),
        ProjectBoardColumnV1::Done => ("Production complete.".to_owned(), 0),
    };

    ProjectBoardProjectionV1 {
        column,
        summary,
        actionable_count,
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StateStore, Workspace};

    fn job(id: &str, status: StepStatus) -> Job {
        Job {
            job_id: id.to_owned(),
            project_id: "prj-1".to_owned(),
            step: "visual".to_owned(),
            unit: id.to_owned(),
            status,
            input_hash: "hash".to_owned(),
            selected_attempt: None,
            selected_artifact: None,
        }
    }

    fn step(id: &str, status: StepStatus) -> WorkflowStep {
        WorkflowStep {
            step_id: id.to_owned(),
            project_id: "prj-1".to_owned(),
            step: "script".to_owned(),
            unit: id.to_owned(),
            status,
            input_hash: None,
        }
    }

    #[test]
    fn maps_display_statuses_to_requested_seven_columns() {
        let cases = [
            (ProjectDisplayStatus::Draft, ProjectBoardColumnV1::Ideas),
            (
                ProjectDisplayStatus::Preparing,
                ProjectBoardColumnV1::Preparing,
            ),
            (
                ProjectDisplayStatus::NeedsReview,
                ProjectBoardColumnV1::NeedsReview,
            ),
            (
                ProjectDisplayStatus::GpuPartial,
                ProjectBoardColumnV1::NeedsReview,
            ),
            (
                ProjectDisplayStatus::GpuReady,
                ProjectBoardColumnV1::GpuReady,
            ),
            (
                ProjectDisplayStatus::GpuRunning,
                ProjectBoardColumnV1::GpuRunning,
            ),
            (
                ProjectDisplayStatus::ReadyForEdit,
                ProjectBoardColumnV1::ReadyToEdit,
            ),
            (ProjectDisplayStatus::Done, ProjectBoardColumnV1::Done),
        ];

        for (status, expected) in cases {
            assert_eq!(
                project_board_projection_v1(status, &[], &[]).column,
                expected
            );
        }
    }

    #[test]
    fn gpu_partial_becomes_actionable_needs_review_summary() {
        let projection = project_board_projection_v1(
            ProjectDisplayStatus::GpuPartial,
            &[
                job("job-a", StepStatus::Retryable),
                job("job-b", StepStatus::Retryable),
                job("job-c", StepStatus::Succeeded),
            ],
            &[],
        );

        assert_eq!(projection.column, ProjectBoardColumnV1::NeedsReview);
        assert_eq!(projection.summary, "2 retryable GPU jobs.");
        assert_eq!(projection.actionable_count, 2);
    }

    #[test]
    fn preparing_summary_uses_canonical_unfinished_steps() {
        let projection = project_board_projection_v1(
            ProjectDisplayStatus::Preparing,
            &[],
            &[
                step("s1", StepStatus::Succeeded),
                step("s2", StepStatus::Ready),
                step("s3", StepStatus::NotReady),
            ],
        );

        assert_eq!(projection.summary, "2 preparation steps remaining.");
        assert_eq!(projection.actionable_count, 2);
    }

    #[test]
    fn board_reconstructs_after_data_root_move_in_read_only_mode() {
        let parent = tempfile::tempdir().unwrap();
        let source = parent.path().join("source");
        let moved = parent.path().join("moved");
        let workspace = Workspace::create(&source).unwrap();
        let mut store = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = store
            .create_project_with_studio_pack("Portable Board", Some("christian-cinematic"))
            .unwrap();
        let job = store
            .create_job(&project.id, "visual", "SC01", "input-hash")
            .unwrap();
        let attempt = store.start_attempt(&job.job_id, Some("worker-a")).unwrap();
        store
            .finish_attempt_failure(&attempt.attempt_id, "NETWORK_TIMEOUT")
            .unwrap();

        let before_status = store.derive_project_status(&project.id).unwrap();
        let before = project_board_projection_v1(
            before_status,
            &store.list_project_jobs(&project.id).unwrap(),
            &store.list_project_steps(&project.id).unwrap(),
        );
        assert_eq!(before.column, ProjectBoardColumnV1::NeedsReview);
        drop(store);
        drop(workspace);

        std::fs::rename(&source, &moved).unwrap();
        let reopened = Workspace::open(&moved).unwrap();
        let read_only = StateStore::open_read_only(reopened.sqlite_path()).unwrap();
        let loaded = read_only.get_project(&project.id).unwrap();
        let after_status = read_only.derive_project_status(&project.id).unwrap();
        let after = project_board_projection_v1(
            after_status,
            &read_only.list_project_jobs(&project.id).unwrap(),
            &read_only.list_project_steps(&project.id).unwrap(),
        );

        assert_eq!(loaded.studio_pack.as_deref(), Some("christian-cinematic"));
        assert_eq!(after, before);
        assert!(read_only.update_project_title(&project.id, "Forbidden").is_err());
    }

    #[test]
    fn interrupted_running_job_reconciles_into_review_column() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("data")).unwrap();
        let mut store = StateStore::open(workspace.sqlite_path()).unwrap();
        let project = store.create_project("Resume Board").unwrap();
        let job = store
            .create_job(&project.id, "voice", "S01", "input-hash")
            .unwrap();
        store.start_attempt(&job.job_id, Some("remote-gpu")).unwrap();

        let running = project_board_projection_v1(
            store.derive_project_status(&project.id).unwrap(),
            &store.list_project_jobs(&project.id).unwrap(),
            &[],
        );
        assert_eq!(running.column, ProjectBoardColumnV1::GpuRunning);

        drop(store);
        let mut reopened = StateStore::open(workspace.sqlite_path()).unwrap();
        reopened.reconcile_interrupted_jobs().unwrap();
        let resumed = project_board_projection_v1(
            reopened.derive_project_status(&project.id).unwrap(),
            &reopened.list_project_jobs(&project.id).unwrap(),
            &[],
        );

        assert_eq!(resumed.column, ProjectBoardColumnV1::NeedsReview);
        assert_eq!(resumed.summary, "1 retryable GPU job.");
    }

    #[test]
    fn ready_to_edit_points_to_existing_production_pack() {
        let projection =
            project_board_projection_v1(ProjectDisplayStatus::ReadyForEdit, &[], &[]);

        assert_eq!(projection.column, ProjectBoardColumnV1::ReadyToEdit);
        assert_eq!(
            projection.summary,
            "Open the Production Pack and continue creative editing."
        );
    }
}
