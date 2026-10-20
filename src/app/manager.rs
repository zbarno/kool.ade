//! Independent project-manager updates; task workers never write into main chat.
mod catalog;
mod dependency;
mod dispositions;
mod prompt;
mod telemetry;
pub(crate) const MAX_PLAN_ARBITRATION_ATTEMPTS: usize = 2;
pub(crate) use dependency::{DependencyReview, DependencyTriage, enforce_policy};
pub(crate) use dispositions::has_unpromoted_nonblocking_findings;
#[cfg(test)]
pub(super) use dispositions::persist_plan_dispositions;
pub(crate) use dispositions::{
    CodeReviewArbitrationRequest, approve_code_review_risk, finish_code_review_arbitration,
    finish_code_review_repair, mark_code_review_repair_failed, next_code_review_arbitration,
    next_code_review_repair,
};
pub(super) use dispositions::{
    has_pending_plan_contest, has_pending_plan_repair, next_plan_arbitration, next_plan_repair,
    persist_dispositions,
};

use crate::harness::{AiHarness, LiveProgress, PlanningRequest};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

pub use activity::{PATROL_COOLDOWN, WorkspaceActivity, patrol_manager_due, patrol_note_due};
mod activity;

pub struct Manager {
    progress: mpsc::Receiver<LiveProgress>,
    result: mpsc::Receiver<Result<ManagerOutput, String>>,
    cancel: Arc<AtomicBool>,
    prompt_manifest: Option<crate::harness::prompt_catalog::PromptManifest>,
}

pub(super) struct ManagerOutput {
    pub summary: String,
    pub proposed_actions: Vec<crate::harness::prompt_catalog::ManagerActionV1>,
    pub opportunities: Vec<crate::harness::prompt_catalog::OpportunityProposalV1>,
    pub evidence_hashes: std::collections::HashMap<String, String>,
    pub planning_revision: String,
    pub code_review_targets:
        std::collections::HashMap<String, catalog::code_reviews::FindingTarget>,
}

pub(super) fn persist_output(project: &super::session::Project, output: &ManagerOutput) -> String {
    let messages = catalog::persist_opportunities(project, output);
    if messages.is_empty() {
        return output.summary.clone();
    }
    format!("{}\n\n{}", output.summary, messages.join("\n"))
}
impl Manager {
    pub(super) fn prompt_body(project: &super::session::Project, events: &[String]) -> String {
        prompt::build(project, events)
    }

    pub fn start(
        project: &super::session::Project,
        events: &[String],
        harness: Box<dyn AiHarness>,
    ) -> Self {
        let (tx, progress) = mpsc::channel();
        let (done, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let prepared = catalog::render(project, events);
        let evidence_hashes = prepared
            .as_ref()
            .map(|prompt| prompt.evidence_hashes.clone())
            .unwrap_or_default();
        let planning_revision = prepared
            .as_ref()
            .map(|prompt| prompt.planning_revision.clone())
            .unwrap_or_default();
        let review_targets = prepared
            .as_ref()
            .map(|prompt| prompt.review_targets.clone())
            .unwrap_or_default();
        let code_review_targets = prepared
            .as_ref()
            .map(|prompt| prompt.code_review_targets.clone())
            .unwrap_or_default();
        let prompt_body = prepared
            .as_ref()
            .map(|prompt| prompt.rendered.user.clone())
            .unwrap_or_default();
        let system_instructions = prepared
            .as_ref()
            .map(|prompt| prompt.rendered.system.clone())
            .unwrap_or_default();
        let prompt_manifest = prepared
            .as_ref()
            .ok()
            .map(|prompt| prompt.rendered.manifest.clone());
        let _ = tx.send(LiveProgress {
            prompt_manifest: prompt_manifest.clone(),
            activity: Some("Project manager prompt prepared".into()),
            ..Default::default()
        });
        let manager_task_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let execution_workspace = manager_execution_workspace(project, &manager_task_id);
        let request = PlanningRequest {
            mode: crate::harness::ExecutionMode::Planning,
            task_id: Some(manager_task_id.clone()),
            execution_workspace,
            destination_sha: None,
            prompt_manifest: prompt_manifest.clone(),
            reasoning_level: "xhigh".into(),
            telemetry_phase: None,
            repo_root: project.state.repo_root.clone(),
            runtime_config_source: None,
            prompt_body,
            system_instructions,
            timeout: crate::core::turn::configured_turn_timeout(),
            progress_tx: tx,
            cancel: cancel.clone(),

            authorization_verifier: None,
            review_identity: None,
        };
        let route = harness.route_provenance(&request);
        let harness_label = harness.label();
        let project_root = project.state.repo_root.clone();
        let execution_id = manager_task_id;
        if request.execution_workspace.is_none() {
            let message = "Kool.ad/e could not bind the current project revision to a read-only Manager workspace. Repair or refresh the project Git checkout, then retry. Manager did not invoke a provider or run repository commands.";
            let failure = crate::error::AppError::HarnessExecutionFailed {
                failure: Box::new(crate::harness::HarnessExecutionFailure {
                    provider_id: route
                        .as_ref()
                        .map(|route| route.provider_id.clone())
                        .unwrap_or_else(|| "unknown".into()),
                    role: crate::execution::contract::HarnessRole::Manager,
                    mode: request.mode,
                    target: None,
                    category: crate::harness::HarnessFailureCategory::ToolBridgeUnavailable,
                    provider_code: None,
                    sanitized_message: message.into(),
                    actionable_fix: Some(
                        "Repair or reconnect the selected project Git checkout before retrying Manager.".into(),
                    ),
                    retryable: false,
                }),
            };
            let detail = failure.detail();
            telemetry::persist_invocation(
                &project_root,
                &harness_label,
                route.as_ref(),
                &request,
                &execution_id,
                telemetry::InvocationOutcome {
                    status: "blocked",
                    failure_category: Some("tool_bridge_unavailable"),
                    failure_message: Some(&detail),
                },
            );
            let _ = done.send(Err(detail));
            return Self {
                progress,
                result,
                cancel,
                prompt_manifest,
            };
        }
        std::thread::spawn(move || {
            let prepared = match prepared {
                Ok(prepared) => prepared,
                Err(error) => {
                    let _ = done.send(Err(error));
                    return;
                }
            };
            let mut outcome = Err("Project manager did not complete".into());
            let mut failure_category = None;
            let mut failure_message = None;
            for attempt in 0..2 {
                if request.cancel.load(Ordering::Relaxed) {
                    failure_category = Some("cancelled".into());
                    failure_message = Some("Manager invocation was cancelled.".into());
                    outcome = Err("Manager invocation was cancelled.".into());
                    break;
                }
                let response = match harness.execute(&request) {
                    Ok(response) => response,
                    Err(error) => {
                        let retryable = matches!(
                            &error,
                            crate::error::AppError::HarnessExecutionFailed {
                                failure,
                            }
                                if failure.retryable
                        );
                        let detail = error.detail();
                        failure_category = Some(telemetry::failure_category(&error));
                        failure_message = Some(crate::error::redact_secrets(&detail));
                        outcome = Err(detail);
                        if !retryable || attempt == 1 {
                            break;
                        }
                        continue;
                    }
                };
                outcome = catalog::decode(
                    &response.final_text,
                    &prepared.validation,
                    evidence_hashes.clone(),
                    planning_revision.clone(),
                    &review_targets,
                    &code_review_targets,
                );
                if outcome.is_ok() {
                    failure_category = None;
                    failure_message = None;
                    break;
                }
                failure_category = Some("invalid_response".into());
                failure_message = outcome
                    .as_ref()
                    .err()
                    .map(|detail| crate::error::redact_secrets(detail));
            }
            telemetry::persist_invocation(
                &project_root,
                &harness_label,
                route.as_ref(),
                &request,
                &execution_id,
                telemetry::InvocationOutcome {
                    status: if outcome.is_ok() {
                        "completed"
                    } else {
                        "failed"
                    },
                    failure_category: failure_category.as_deref(),
                    failure_message: failure_message.as_deref(),
                },
            );
            let _ = done.send(outcome);
        });
        Self {
            progress,
            result,
            cancel,
            prompt_manifest,
        }
    }
    pub fn progress(&self) -> Option<LiveProgress> {
        self.progress.try_recv().ok().map(|mut progress| {
            if progress.prompt_manifest.is_none() {
                progress.prompt_manifest = self.prompt_manifest.clone();
            }
            progress
        })
    }
    pub(super) fn result(&self) -> Option<Result<ManagerOutput, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Project-manager process ended unexpectedly".into()))
            }
        }
    }
}

fn manager_execution_workspace(
    project: &super::session::Project,
    task_id: &str,
) -> Option<crate::execution::contract::WorkspaceRef> {
    let repo_root = &project.state.repo_root;
    let root_metadata = std::fs::symlink_metadata(repo_root).ok()?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return None;
    }
    let git_metadata = std::fs::symlink_metadata(repo_root.join(".git")).ok()?;
    if git_metadata.file_type().is_symlink() || !(git_metadata.is_dir() || git_metadata.is_file()) {
        return None;
    }
    let root = repo_root.canonicalize().ok()?;
    let mut git = crate::core::planning_sync::validated_git_command(&root).ok()?;
    let output = git.args(["rev-parse", "--verify", "HEAD"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let source_sha = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    if !crate::execution::contract::is_valid_sha(&source_sha) {
        return None;
    }
    Some(crate::execution::contract::WorkspaceRef {
        project_id: project
            .state
            .planning_store
            .project_id
            .hyphenated()
            .to_string(),
        task_id: task_id.to_owned(),
        source_sha,
    })
}
impl Drop for Manager {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests;
