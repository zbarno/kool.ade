use super::*;
use crate::harness::{HarnessOutcome, PlanningRequest};
use std::path::PathBuf;
struct StaticHarness(String);
impl AiHarness for StaticHarness {
    fn label(&self) -> String {
        "reconciliation fixture".into()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok("fixture".into())
    }
    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, crate::error::AppError> {
        assert_eq!(request.mode, ExecutionMode::Reconciliation);
        assert!(
            request
                .prompt_body
                .contains("MERGED IMPLEMENTATION EVIDENCE")
        );
        Ok(HarnessOutcome {
            final_text: self.0.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}
fn git(repo: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}
fn fixture() -> (PathBuf, PlannerState, Candidate, String) {
    let repo = std::env::temp_dir().join(format!(
        "koolade_reconcile_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(repo.join(".koolade-packet/planning/changes/CHG-001-search")).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.name", "Fixture"]);
    git(&repo, &["config", "user.email", "fixture@example.test"]);
    let legacy = crate::artifacts::spec_doc::bootstrap_template("Demo");
    std::fs::create_dir_all(repo.join("planning")).unwrap();
    std::fs::write(repo.join("planning/specification.md"), &legacy).unwrap();
    crate::artifacts::product_docs::migrate(&repo, &legacy).unwrap();
    std::fs::create_dir_all(repo.join(".koolade-packet/planning")).unwrap();
    let feature = "# CHG-001: Search\n\n**Status:** Implementing\n\n## Intent\n\nSave searches.\n\n## Current Behavior\n\nNo persistence.\n\n## Desired Behavior\n\nQueries persist.\n\n## Scope\n\nSearch.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nQueries persist.\n\n## Decisions and Assumptions\n\nUse local store.\n\n## Acceptance Criteria\n\nQuery survives restart.\n".to_string();
    std::fs::write(
        repo.join(".koolade-packet/planning/changes/CHG-001-search/specification.md"),
        &feature,
    )
    .unwrap();
    let mut workflow = workflow::Workflow::default();
    workflow
        .approved_features
        .insert("CHG-001".into(), workflow::feature_contract(&feature));
    crate::artifacts::task_docs::save_workflow(&repo, &workflow).unwrap();
    std::fs::create_dir_all(repo.join(".koolade-packet/planning/tasks/search")).unwrap();
    let ticket = "# Save query\n\nFeature ID: CHG-001\nRepository: root\n";
    std::fs::write(
        repo.join(".koolade-packet/planning/tasks/search/001-save-query.md"),
        ticket,
    )
    .unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "baseline"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    std::fs::write(repo.join("implemented.txt"), "search queries persist\n").unwrap();
    git(&repo, &["add", "implemented.txt"]);
    git(&repo, &["commit", "-qm", "implement search"]);
    let merged = git(&repo, &["rev-parse", "HEAD"]);
    let state = PlannerState::load(&repo).unwrap();
    let task: crate::core::implementation::Implementation = serde_json::from_value(serde_json::json!({
            "ticket":".koolade-packet/planning/tasks/search/001-save-query.md","ticket_text":ticket,"branch":"koolade/task",
            "base":"main","base_commit":base,"worktree":repo,"status":"completed","detail":"",
            "pr_url":null,"verified_head":merged,"merged_commit":merged
        })).unwrap();
    let contract = crate::core::contract_snapshot::BatchContract {
        feature_id: "CHG-001".into(),
        feature_specification: feature.clone(),
        product_modules: [(
            "current-capabilities".to_string(),
            std::fs::read_to_string(
                repo.join(".koolade-packet/planning/product/current-capabilities.md"),
            )
            .unwrap(),
        )]
        .into(),
        repository_bases: Default::default(),
        configuration: String::new(),
    };
    let candidate = Candidate {
        feature_id: "CHG-001".into(),
        batch_directory: ".koolade-packet/planning/tasks/search".into(),
        contract,
        tasks: vec![task],
    };
    (repo, state, candidate, feature)
}

#[test]
fn completed_response_is_scoped_and_preserves_approved_intent() {
    let (repo, state, candidate, feature) = fixture();
    let merged = candidate.tasks[0].merged_commit.as_deref().unwrap();
    let updated_feature = feature.replace(
        "**Status:** Implementing",
        &format!("**Status:** Implemented\n\n**Implementation:** {merged}"),
    );
    let product = candidate.contract.product_modules["current-capabilities"].clone()
        + "\nCurrent search queries persist.\n";
    let env: crate::harness::responses::ReconciliationResponse =
            serde_json::from_value(serde_json::json!({
                "schema_version":2,"assistant_message":"Reconciled merged search behavior.",
                "document_updates":[{"document_id":"feature:CHG-001","content":updated_feature,"status":"implemented"},
                    {"document_id":"product:current-capabilities","content":product}]
            }))
            .unwrap();
    assert!(validate_response(&state, &candidate, &env).is_ok());
    let bad: crate::harness::responses::ReconciliationResponse = serde_json::from_value(serde_json::json!({
            "schema_version":2,"assistant_message":"Reconciled.",
            "document_updates":[{"document_id":"feature:CHG-001","content":feature.replace("**Status:** Implementing", "**Status:** Implemented")},
                {"document_id":"product:02-scope","content":"## 2. Scope\n\nWrong module.\n"}]
        })).unwrap();
    assert!(validate_response(&state, &candidate, &bad).is_err());
    let _ = std::fs::remove_dir_all(repo);
}

#[test]
fn material_discrepancy_requires_board_item_and_preserves_product() {
    let (repo, state, candidate, feature) = fixture();
    let revised = feature.replace("**Status:** Implementing", "**Status:** Reconciliation");
    let discrepancy: crate::harness::responses::ReconciliationResponse = serde_json::from_value(serde_json::json!({
            "schema_version":2,"assistant_message":"Found a mismatch.",
            "document_updates":[{"document_id":"feature:CHG-001","content":revised,"status":"reconciliation"}],
            "open_items_added":[{"kind":"Assumption","priority":"Normal","authority":"Review",
                "category":"General","assigned_to":"All","feature_id":"CHG-001",
                "question":"Reconciliation found missing persisted queries; approve a corrective task?",
                "reason":"Merged commit does not match the approved feature.",
                "recommendation":"Create a corrective task before product truth is updated.",
                "evidence":"Merged implementation has no query persistence."}]
        })).unwrap();
    assert!(validate_response(&state, &candidate, &discrepancy).is_ok());
    let _ = std::fs::remove_dir_all(repo);
}

#[path = "tests/merged_results.rs"]
mod merged_results;
