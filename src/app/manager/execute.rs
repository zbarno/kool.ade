use super::{
    ManagerOutput,
    catalog::{self, ManagerPrompt},
};
use crate::harness::{AiHarness, PlanningRequest};
use std::sync::mpsc;

pub(super) fn launch(
    harness: Box<dyn AiHarness>,
    request: PlanningRequest,
    prepared: Result<ManagerPrompt, String>,
    product_prepared: Result<Option<ManagerPrompt>, String>,
    done: mpsc::Sender<Result<ManagerOutput, String>>,
) {
    std::thread::spawn(move || {
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = done.send(Err(error));
                return;
            }
        };
        let mut output = match run_prompt(&*harness, &request, &prepared) {
            Ok(output) => output,
            Err(error) => {
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
                if let Some(repo_root) = product_prompt.repo_root.clone() {
                    product_request.repo_root = repo_root;
                }
                match run_prompt(&*harness, &product_request, &product_prompt) {
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
        let _ = done.send(Ok(output));
    });
}

fn run_prompt(
    harness: &dyn AiHarness,
    request: &PlanningRequest,
    prepared: &ManagerPrompt,
) -> Result<ManagerOutput, String> {
    let mut outcome = Err("Project manager did not complete".to_owned());
    for _ in 0..2 {
        if request.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Project manager was cancelled".into());
        }
        outcome = harness
            .execute(request)
            .map_err(|error| error.detail())
            .and_then(|response| {
                catalog::decode(
                    &response.final_text,
                    &prepared.validation,
                    prepared.evidence_hashes.clone(),
                    prepared.evidence_confidentialities.clone(),
                )
            });
        if outcome.is_ok() {
            break;
        }
    }
    outcome
}
