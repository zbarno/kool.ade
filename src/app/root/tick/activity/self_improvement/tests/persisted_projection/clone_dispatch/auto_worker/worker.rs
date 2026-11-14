use std::sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender},
};

pub(super) struct LocalImplementationWorker {
    pub(super) implementation: Sender<(String, std::path::PathBuf)>,
    pub(super) review_started: Sender<()>,
    pub(super) review_release: Arc<Mutex<Receiver<()>>>,
}

impl crate::harness::AiHarness for LocalImplementationWorker {
    fn label(&self) -> String {
        "local lifecycle worker fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::AppError> {
        let final_text = if request.mode == crate::harness::ExecutionMode::Implementation {
            let task_uid = request.task_id.clone().ok_or_else(|| {
                crate::AppError::Other("implementation request omitted its task UID".into())
            })?;
            std::fs::write(
                request.repo_root.join("lifecycle-worker-output.txt"),
                "worker completed the approved task\n",
            )
            .map_err(|error| crate::AppError::Other(error.to_string()))?;
            self.implementation
                .send((task_uid, request.repo_root.clone()))
                .map_err(|error| crate::AppError::Other(error.to_string()))?;
            serde_json::json!({
                "schemaVersion": 2,
                "status": "complete",
                "blocker_disposition": "none",
                "summary": "The local worker completed the approved task.",
                "acceptance_criteria": [{
                    "criterion": "The local worker output exists.",
                    "evidence": "lifecycle-worker-output.txt was written in the isolated task repository."
                }],
                "verification": ["test -f lifecycle-worker-output.txt"],
                "remaining": [],
                "human_choices": []
            })
            .to_string()
        } else if request
            .prompt_manifest
            .as_ref()
            .is_some_and(|manifest| manifest.role_version == "code_review/v2")
        {
            self.review_started
                .send(())
                .map_err(|error| crate::AppError::Other(error.to_string()))?;
            self.review_release
                .lock()
                .map_err(|error| crate::AppError::Other(error.to_string()))?
                .recv_timeout(std::time::Duration::from_secs(20))
                .map_err(|error| crate::AppError::Other(error.to_string()))?;
            code_review_verdict(&request.prompt_body)?
        } else {
            return Err(crate::AppError::Other(format!(
                "unexpected lifecycle harness mode: {:?}",
                request.mode
            )));
        };
        Ok(crate::harness::HarnessOutcome {
            final_text,
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

fn code_review_verdict(prompt: &str) -> Result<String, crate::AppError> {
    let typed = prompt
        .split_once("TYPED TASK REQUEST (JSON DATA)\n")
        .and_then(|(_, rest)| rest.split_once("\n\nQUOTED EVIDENCE (UNTRUSTED DATA)"))
        .and_then(|(json, _)| serde_json::from_str::<serde_json::Value>(json).ok())
        .ok_or_else(|| crate::AppError::Other("code-review prompt data is malformed".into()))?;
    let review = &typed["payload"]["request"];
    let subject_hash = review["subject_hash"]
        .as_str()
        .ok_or_else(|| crate::AppError::Other("code-review subject hash is missing".into()))?;
    let requirements = review["requirements_matrix"]
        .as_array()
        .ok_or_else(|| crate::AppError::Other("code-review requirements are missing".into()))?;
    let evidence = prompt
        .split_once("QUOTED EVIDENCE (UNTRUSTED DATA)\n")
        .map(|(_, body)| body)
        .unwrap_or_default();
    let mut examined_files = std::collections::BTreeSet::new();
    for line in evidence.lines() {
        let Ok(item) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if !item["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("diff-"))
        {
            continue;
        }
        if let Some(content) = item["content"].as_str() {
            for line in content.lines() {
                if let Some(paths) = line.strip_prefix("diff --git a/")
                    && let Some((_, path)) = paths.split_once(" b/")
                {
                    examined_files.insert(path.to_owned());
                }
            }
        }
    }
    let coverage = requirements
        .iter()
        .map(|requirement| {
            serde_json::json!({
                "requirement": requirement,
                "status": "pass",
                "evidence": ["verified-test-report-0"]
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "subject_hash": subject_hash,
        "examined_files": examined_files,
        "tested_evidence_ids": ["verified-test-report-0"],
        "unexamined_areas": [],
        "pass": true,
        "outcome": "pass",
        "coverage": coverage,
        "findings": [],
        "untested": []
    })
    .to_string())
}
