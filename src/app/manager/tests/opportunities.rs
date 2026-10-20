use super::*;
use crate::harness::{AiHarness, HarnessOutcome};
use std::{path::Path, process::Command, thread, time::Instant};

struct OpportunityHarness(String);

impl AiHarness for OpportunityHarness {
    fn label(&self) -> String {
        "opportunity-fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok("fixture ready".into())
    }

    fn execute(
        &self,
        _request: &crate::harness::PlanningRequest,
    ) -> Result<HarnessOutcome, crate::error::AppError> {
        Ok(HarnessOutcome {
            final_text: self.0.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git is installed");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn proposal_json(repository: &str) -> serde_json::Value {
    serde_json::json!({
        "origin": "code_inspection",
        "kind": "quality",
        "target": "active_user_project",
        "target_repository": repository,
        "target_repository_ref": "root",
        "related_feature_uids": [],
        "related_task_uids": [],
        "observation_revision": "1",
        "evidence_refs": ["workspace-summary", "generic-capability-inventory"],
        "problem_or_opportunity": "Repeated verification failures cost time",
        "user_value": "Faster, more reliable iteration",
        "expected_outcome": "Fewer repeated failures",
        "possible_approaches": ["Improve preflight diagnostics"],
        "recommended_approach": "Add a deterministic preflight check",
        "alternatives": "Keep current diagnostics",
        "estimated_scope": "small",
        "impact": "Shorter repair cycles",
        "cost_or_risk": "Small maintenance cost",
        "affected_systems": ["verification"],
        "current_capability_state": "Generic verification exists but lacks this check",
        "generic_workaround": "Inspect the task log manually",
        "proposed_approach": "Add the preflight check to the existing flow",
        "dedup_fingerprint": "b".repeat(64),
        "inference": "Repeated failures suggest a recurring diagnostic gap",
        "confidence": "medium",
        "speculative": false,
        "generic_capability_check": "Checked generic-capability-inventory: the current generic path was inspected",
        "deduplication_basis": "No matching open proposal was supplied"
    })
}

pub(super) fn initialized_repository() -> (tempfile::TempDir, crate::app::session::Project, String)
{
    let temp = tempfile::tempdir().expect("temporary project directory");
    let root = temp.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Fixture User"]);
    git(root, &["config", "user.email", "fixture@example.invalid"]);
    git(
        root,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/project.git",
        ],
    );
    std::fs::write(root.join("README.md"), "fixture\n").unwrap();
    git(root, &["add", "README.md"]);
    git(root, &["commit", "-qm", "fixture"]);
    let project = crate::app::session::test_support::project_from(root);
    (
        temp,
        project,
        "https://github.com/example/project.git".into(),
    )
}

#[test]
fn manager_generation_persists_only_valid_opportunity_proposals() {
    let (_temp, project, repository) = initialized_repository();
    let output = serde_json::json!({
        "summary": "A small improvement may reduce recurring failures.",
        "proposed_actions": [],
        "opportunities": [proposal_json(&repository)],
    })
    .to_string();
    let manager = Manager::start(
        &project,
        &["event-1".into()],
        Box::new(OpportunityHarness(output)),
    );
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    let result = loop {
        if let Some(result) = manager.result() {
            break result.expect("validated manager output");
        }
        assert!(Instant::now() < deadline, "manager harness timed out");
        thread::sleep(std::time::Duration::from_millis(5));
    };

    let summary = persist_output(&project, &result);
    assert!(summary.contains("New improvement proposal"));
    let store = crate::core::opportunity::OpportunityStore::new(&project.state.planning_store);
    let proposals = store.list().expect("load saved proposals");
    assert_eq!(proposals.len(), 1);
    assert_eq!(
        proposals[0].status,
        crate::core::opportunity::OpportunityStatusV1::AwaitingDecision
    );
    assert!(store.list_decisions().expect("load decisions").is_empty());
}

#[test]
fn manager_prompt_supplies_public_product_evidence_only_for_linked_product_repo() {
    let (_temp, mut project, _) = initialized_repository();
    let events = ["manager-refresh".to_owned()];
    let unlinked = super::super::catalog::render(&project, &events).unwrap();
    assert!(
        !unlinked
            .validation
            .evidence_ids
            .contains("koolade-product-opportunity-contract")
    );

    project.state.repositories.repositories[0].remote = "https://github.com/zbarno/kool.ade".into();
    let linked = super::super::catalog::render(&project, &events).unwrap();
    assert!(
        linked
            .validation
            .public_evidence_ids
            .contains("koolade-product-opportunity-contract")
    );
    assert_eq!(
        linked
            .validation
            .evidence_repository_refs
            .get("koolade-product-opportunity-contract")
            .map(String::as_str),
        Some("root")
    );
    assert!(
        linked
            .rendered
            .user
            .contains("koolade-product-opportunity-contract")
    );
}

#[test]
fn manager_rejects_opportunity_for_unlinked_repository() {
    let (_temp, project, _repository) = initialized_repository();
    let output = serde_json::json!({
        "summary": "Candidate proposal.",
        "proposed_actions": [],
        "opportunities": [proposal_json("https://github.com/example/unlinked.git")],
    })
    .to_string();
    let manager = Manager::start(
        &project,
        &["event-1".into()],
        Box::new(OpportunityHarness(output)),
    );
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    let result = loop {
        if let Some(result) = manager.result() {
            break result;
        }
        assert!(Instant::now() < deadline, "manager harness timed out");
        thread::sleep(std::time::Duration::from_millis(5));
    };

    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("unlinked repository identity must fail validation"),
    };
    assert!(!error.is_empty(), "validation failure should be explained");
    let store = crate::core::opportunity::OpportunityStore::new(&project.state.planning_store);
    assert!(store.list().expect("load proposals").is_empty());
}
