//! One bounded, read-only SpecReconciler pass over an exact reviewed task.
use crate::app::session::Project;
use crate::core::lifecycle::TaskRecord;
use crate::harness::AiHarness;
use sha2::Digest as _;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

pub(crate) struct SpecReconciliation {
    pub(crate) task_uid: String,
    pub(crate) expected_revision: u64,
    pub(crate) spec_sha256: String,
    result: mpsc::Receiver<Result<SpecReconciliationOutcome, String>>,
    cancel: Arc<AtomicBool>,
}

pub(crate) struct SpecReconciliationOutcome {
    pub(crate) task_uid: String,
    pub(crate) expected_revision: u64,
    pub(crate) spec_sha256: String,
    pub(crate) proposal: crate::harness::prompt_catalog::SpecDeltaV1,
    pub(crate) prompt: crate::harness::prompt_catalog::PromptManifest,
}

impl SpecReconciliation {
    pub(crate) fn start(
        project: &Project,
        task: &TaskRecord,
        harness: Box<dyn AiHarness>,
    ) -> Result<Self, String> {
        let (request, context, spec_sha256) = prepare(project, task)?;
        let task_uid = task.task_uid.clone();
        let expected_revision = task.revision;
        let prompt = request
            .prompt_manifest
            .clone()
            .ok_or_else(|| "SpecReconciler prompt manifest is missing".to_owned())?;
        let (done, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let outcome_spec_sha256 = spec_sha256.clone();
        std::thread::spawn(move || {
            let output = if worker_cancel.load(Ordering::Relaxed) {
                Err("specification reconciliation was cancelled".into())
            } else {
                harness
                    .execute(&request)
                    .map_err(|error| error.detail())
                    .and_then(|response| {
                        match crate::harness::prompt_catalog::validate_output_with_manifest(
                            &prompt,
                            &response.final_text,
                            &context,
                        )? {
                            crate::harness::prompt_catalog::RoleOutputV1::SpecDelta(proposal) => {
                                validate_delta(&proposal, &context.required_requirements)?;
                                Ok(SpecReconciliationOutcome {
                                    task_uid,
                                    expected_revision,
                                    spec_sha256: outcome_spec_sha256,
                                    proposal,
                                    prompt,
                                })
                            }
                            _ => Err("SpecReconciler returned an unexpected schema".into()),
                        }
                    })
            };
            let _ = done.send(output);
        });
        Ok(Self {
            task_uid: task.task_uid.clone(),
            expected_revision: task.revision,
            spec_sha256,
            result,
            cancel,
        })
    }

    pub(crate) fn take_result(&self) -> Option<Result<SpecReconciliationOutcome, String>> {
        self.result.try_recv().ok()
    }
}

impl Drop for SpecReconciliation {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

mod approve;
mod prepare;
use prepare::prepare;

pub(crate) use approve::apply_approved_delta;

fn evidence_item(
    id: &str,
    workspace_uid: &str,
    source: &str,
    content: &str,
) -> Result<crate::harness::prompt_catalog::EvidenceItem, String> {
    use crate::harness::prompt_catalog::{EvidenceConfidentiality, EvidenceItem, EvidenceOrigin};
    if id.len() > 128 || content.len() > 16 * 1024 {
        return Err(format!(
            "{source} evidence exceeds the SpecReconciler prompt limit"
        ));
    }
    Ok(EvidenceItem {
        id: id.to_owned(),
        workspace_uid: workspace_uid.to_owned(),
        origin: EvidenceOrigin::Application,
        confidentiality: EvidenceConfidentiality::WorkspacePrivate,
        source: source.to_owned(),
        content: content.to_owned(),
    })
}

fn validate_delta(
    proposal: &crate::harness::prompt_catalog::SpecDeltaV1,
    requirement_ids: &[String],
) -> Result<(), String> {
    if proposal.deltas.len() > 128 {
        return Err("SpecReconciler proposed too many specification deltas".into());
    }
    let mut seen = std::collections::HashSet::new();
    for delta in &proposal.deltas {
        if !requirement_ids.contains(&delta.requirement_id)
            || !seen.insert(&delta.requirement_id)
            || delta.old.trim().is_empty()
            || delta.new.trim().is_empty()
            || delta.old.len() > 8192
            || delta.new.len() > 8192
            || delta.old == delta.new
            || delta.evidence.is_empty()
            || delta.followup_task_ids.len() > 64
            || delta
                .followup_task_ids
                .iter()
                .any(|uid| uid.trim().is_empty() || uid.len() > 128)
        {
            return Err("SpecReconciler proposal is not a unique, bounded, evidence-linked requirement delta".into());
        }
    }
    Ok(())
}

pub(crate) fn spec_binding_hash(spec: Option<&str>) -> Result<String, String> {
    let encoded = serde_json::to_vec(&spec).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", sha2::Sha256::digest(encoded)))
}
