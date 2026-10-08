use super::{Implementation, Runner};
use crate::harness::{ExecutionMode, PlanningRequest};

pub(super) fn build(
    state: &Implementation,
    runner: &Runner,
    prompt: String,
    attempt: usize,
    verification_corrections: usize,
) -> anyhow::Result<PlanningRequest> {
    Ok(PlanningRequest {
        mode: ExecutionMode::Implementation,
        task_id: state
            .task_uid
            .clone()
            .or_else(|| Some(state.ticket.clone())),
        reasoning_level: "medium".into(),
        telemetry_phase: Some(
            if verification_corrections > 0 {
                "qa_verification"
            } else if attempt == 1 {
                "implementation"
            } else {
                "repair"
            }
            .into(),
        ),
        repo_root: state.task_repository.clone(),
        runtime_config_source: runner.runtime_config_source.clone(),
        prompt_body: prompt,
        system_instructions: "You are an implementation agent. Read and follow repository AGENTS.md instructions. Implement, integrate, and verify the whole ticket. Preserve existing work when resuming or correcting a failed report. Return the required JSON report. Report blockers honestly. Git metadata is read-only inside your sandbox: do not run git add or commit. For new files, inspect their contents directly; Kool.ad/e stages them and runs final verification. The application alone manages Git commits, integration, and publication."
            .into(),
        timeout: runner.remaining()?,
        progress_tx: runner.progress.clone(),
        cancel: runner.cancel.clone(),
    })
}
