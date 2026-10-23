use crate::harness::{AiHarness, HarnessOutcome, PlanningRequest};

pub(super) struct PassingPlanReviewer;

impl AiHarness for PassingPlanReviewer {
    fn label(&self) -> String {
        "isolated self-improvement plan reviewer fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }

    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, crate::error::AppError> {
        let typed_request = request
            .prompt_body
            .split_once("TYPED TASK REQUEST (JSON DATA)\n")
            .and_then(|(_, rest)| rest.split_once("\n\nQUOTED EVIDENCE").map(|(json, _)| json))
            .expect("review prompt contains the typed request");
        let typed_request: serde_json::Value = serde_json::from_str(typed_request).unwrap();
        let review = &typed_request["payload"]["request"];
        let subject_hash = review["subject_hash"].as_str().unwrap();
        let evidence_id = typed_request["evidence_manifest"][0]["id"]
            .as_str()
            .expect("review prompt includes the persisted plan snapshot");
        let coverage = review["requirements_matrix"]
            .as_array()
            .expect("review request includes requirement coverage")
            .iter()
            .map(|entry| {
                let requirement = entry
                    .as_str()
                    .unwrap()
                    .split_once(':')
                    .expect("requirement matrix includes its stable ID")
                    .0;
                serde_json::json!({
                    "requirement": requirement,
                    "status": "pass",
                    "evidence": [evidence_id]
                })
            })
            .collect::<Vec<_>>();
        Ok(HarnessOutcome {
            final_text: serde_json::json!({
                "subject_hash": subject_hash,
                "pass": true,
                "outcome": "pass",
                "coverage": coverage,
                "findings": [],
                "untested": []
            })
            .to_string(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}
