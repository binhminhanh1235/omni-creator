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
