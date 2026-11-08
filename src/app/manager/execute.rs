use sha2::Digest;

mod verified_evidence;
pub(super) use verified_evidence::verified_broker_evidence;

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
        let mut context_telemetry = prepared.context_telemetry.clone();
        let has_answerable_source = context_telemetry.selected_source_ids.iter().any(|id| {
            id.starts_with("investigation:")
                || id.starts_with("repository-architecture-profile")
                || id.starts_with("runtime-qa")
                || id.starts_with("published-spec:")
                || id.starts_with("authorized-task-direction:")
                || id.contains("capability")
        });
        context_telemetry.uncertainty_reported = Some(output.uncertainty_reported);
        context_telemetry.answerable_without_user_question = Some(
            has_answerable_source
                && !output.proposed_actions.iter().any(|action| {
                    matches!(
                        &action.kind,
                        crate::harness::prompt_catalog::ManagerActionKind::AskUser
                    )
                }),
        );
        let _ = request.progress_tx.send(crate::harness::LiveProgress {
            manager_context: Some(context_telemetry),
            ..Default::default()
        });
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
    let mut request = request.clone();
    for attempt in 0..2 {
        if request.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            let message = "Project manager was cancelled".to_owned();
            return (
                Err(message.clone()),
                Some("cancelled".into()),
                Some(message),
            );
        }
        let response = match harness.execute(&request) {
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
        let mut validation = prepared.validation.clone();
        let mut hashes = prepared.evidence_hashes.clone();
        let mut confidentiality = prepared.evidence_confidentialities.clone();
        let broker_evidence = request
            .investigation_authorization
            .as_ref()
            .map(verified_broker_evidence)
            .transpose();
        let mut reviewed_repository_evidence = Vec::new();
        let decoded = match broker_evidence {
            Ok(Some(evidence)) => {
                for (id, hash, content) in evidence {
                    if id.starts_with("investigation:")
                        && content.len() <= 2048
                        && reviewed_repository_evidence.len() < 8
                        && let Some(authorization) = request.investigation_authorization.as_ref()
                    {
                        reviewed_repository_evidence.push(
                            crate::domain::chatlog::VerifiedRepositoryEvidence {
                                evidence_id: id.clone(),
                                repository_id: authorization.repository_id.clone(),
                                source_ref: authorization.source_ref.clone(),
                                source_sha: authorization.source_sha.clone(),
                                content_sha256: hash.clone(),
                                excerpt_sha256: format!(
                                    "{:x}",
                                    sha2::Sha256::digest(content.as_bytes())
                                ),
                                content: content.clone(),
                            },
                        );
                    }
                    validation.evidence_ids.insert(id.clone());
                    validation.evidence_content.insert(id.clone(), content);
                    hashes.insert(id.clone(), hash);
                    confidentiality.insert(
                        id,
                        crate::harness::prompt_catalog::EvidenceConfidentiality::WorkspacePrivate,
                    );
                }
                catalog::decode(
                    &response.final_text,
                    &validation,
                    hashes,
                    confidentiality,
                    prepared.planning_revision.clone(),
                    &prepared.review_targets,
                    &prepared.code_review_targets,
                )
            }
            Ok(None) => catalog::decode(
                &response.final_text,
                &validation,
                hashes,
                confidentiality,
                prepared.planning_revision.clone(),
                &prepared.review_targets,
                &prepared.code_review_targets,
            ),
            Err(error) => Err(error),
        };
        outcome = decoded.map(|mut output| {
            output.verified_repository_evidence = reviewed_repository_evidence;
            output
        });
        if outcome.is_ok() {
            failure_category = None;
            failure_message = None;
            break;
        }
        if let Err(detail) = &outcome {
            request.prompt_body.push_str(&format!(
                "\nApplication validation rejected the previous response: {}\nReturn only a corrected ManagerOutputV3 JSON object with exactly these top-level fields: summary, what_exists, how_known, uncertainty, next_action, conflicts, summary_evidence, proposed_actions, and opportunities. Cite only supplied evidence IDs in how_known, summary_evidence, action evidence, and conflict references. If the summary relies on investigation evidence, reproduce one complete sanitized evidence line verbatim and cite its source ID in summary_evidence; add no conclusions to that line. Do not add schema_version, output_schema_version, evidence_refs, or commentary fields. Preserve the original evidence and action restrictions. Empty action arrays are valid; do not invent actions to repair the format.",
                crate::core::context_build::clip_bytes(&crate::error::redact_secrets(detail), 2048)
            ));
        }
        failure_category = Some("invalid_response".into());
        failure_message = outcome
            .as_ref()
            .err()
            .map(|detail| crate::error::redact_secrets(detail));
    }
    (outcome, failure_category, failure_message)
}
