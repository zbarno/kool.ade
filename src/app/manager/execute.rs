use super::{
    ManagerOutput,
    catalog::{self, ManagerPrompt},
};
use crate::harness::{AiHarness, HarnessRouteProvenance, PlanningRequest};
use std::sync::mpsc;

pub(super) struct InvocationContext {
    pub(super) route: Option<HarnessRouteProvenance>,
    pub(super) harness_label: String,
    pub(super) project_root: std::path::PathBuf,
    pub(super) execution_id: String,
}

pub(super) fn launch(
    harness: Box<dyn AiHarness>,
    request: PlanningRequest,
    prepared: Result<ManagerPrompt, String>,
    product_prepared: Result<Option<ManagerPrompt>, String>,
    invocation: InvocationContext,
    done: mpsc::Sender<Result<ManagerOutput, String>>,
) {
    std::thread::spawn(move || {
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                super::telemetry::persist_invocation(
                    &invocation.project_root,
                    &invocation.harness_label,
                    invocation.route.as_ref(),
                    &request,
                    &invocation.execution_id,
                    super::telemetry::InvocationOutcome {
                        status: "blocked",
                        failure_category: Some("prompt_preparation_failed"),
                        failure_message: Some(&crate::error::redact_secrets(&error)),
                    },
                );
                let _ = done.send(Err(error));
                return;
            }
        };
        let (outcome, failure_category, failure_message) =
            run_prompt(&*harness, &request, &prepared);
        let mut output = match outcome {
            Ok(output) => output,
            Err(error) => {
                super::telemetry::persist_invocation(
                    &invocation.project_root,
                    &invocation.harness_label,
                    invocation.route.as_ref(),
                    &request,
                    &invocation.execution_id,
                    super::telemetry::InvocationOutcome {
                        status: "failed",
                        failure_category: failure_category.as_deref(),
                        failure_message: failure_message.as_deref(),
                    },
                );
                let _ = done.send(Err(error));
                return;
            }
        };
        match product_prepared {
            Ok(Some(product_prompt))
                if !request.cancel.load(std::sync::atomic::Ordering::Relaxed) =>
            {
                let mut product_request = request.clone();
                product_request.prompt_manifest = Some(product_prompt.rendered.manifest.clone());
                product_request.prompt_body = product_prompt.rendered.user.clone();
                product_request.system_instructions = product_prompt.rendered.system.clone();
                // Product discovery receives public evidence in an empty cwd and no
                // workspace authority for the linked user's project.
                product_request.mode = crate::harness::ExecutionMode::ReadOnlyAnalysis;
                product_request.task_id = None;
                product_request.execution_workspace = None;
                if let Some(repo_root) = product_prompt.repo_root.clone() {
                    product_request.repo_root = repo_root;
                }
                match run_prompt(&*harness, &product_request, &product_prompt).0 {
                    Ok(mut product_output) => {
                        output
                            .opportunities
                            .append(&mut product_output.opportunities);
                        output
                            .evidence_hashes
                            .extend(product_output.evidence_hashes);
                        output
                            .evidence_confidentialities
                            .extend(product_output.evidence_confidentialities);
                    }
                    Err(_) => {
                        let _ = request.progress_tx.send(crate::harness::LiveProgress {
                            activity: Some(
                                "Public Kool.ad/e opportunity review could not be completed".into(),
                            ),
                            ..Default::default()
                        });
                    }
                }
            }
            Ok(Some(_)) => {}
            Ok(None) => {}
            Err(_) => {
                let _ = request.progress_tx.send(crate::harness::LiveProgress {
                    activity: Some(
                        "Public Kool.ad/e opportunity review could not be prepared".into(),
                    ),
                    ..Default::default()
                });
            }
        }
        super::telemetry::persist_invocation(
            &invocation.project_root,
            &invocation.harness_label,
            invocation.route.as_ref(),
            &request,
            &invocation.execution_id,
            super::telemetry::InvocationOutcome {
                status: "completed",
                failure_category: None,
                failure_message: None,
            },
        );
        let _ = done.send(Ok(output));
    });
}

fn run_prompt(
    harness: &dyn AiHarness,
    request: &PlanningRequest,
    prepared: &ManagerPrompt,
) -> (
    Result<ManagerOutput, String>,
    Option<String>,
    Option<String>,
) {
    let mut outcome = Err("Project manager did not complete".to_owned());
    let mut failure_category = None;
    let mut failure_message = None;
    for attempt in 0..2 {
        if request.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            let message = "Project manager was cancelled".to_owned();
            return (
                Err(message.clone()),
                Some("cancelled".into()),
                Some(message),
            );
        }
        let response = match harness.execute(request) {
            Ok(response) => response,
            Err(error) => {
                let retryable = matches!(
                    &error,
                    crate::error::AppError::HarnessExecutionFailed { failure }
                        if failure.retryable
                );
                let detail = error.detail();
                failure_category = Some(super::telemetry::failure_category(&error));
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
            prepared.evidence_hashes.clone(),
            prepared.evidence_confidentialities.clone(),
            prepared.planning_revision.clone(),
            &prepared.review_targets,
            &prepared.code_review_targets,
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
    (outcome, failure_category, failure_message)
}
