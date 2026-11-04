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
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["branch", "-M", "main"]);
    git(&repo, &["config", "user.name", "Fixture"]);
    git(&repo, &["config", "user.email", "fixture@example.test"]);
    let product = repo.join("docs/koolade/product");
    std::fs::create_dir_all(&product).unwrap();
    std::fs::write(
        product.join("current-capabilities.md"),
        "# Current Capabilities\n\nNo persistence.\n",
    )
    .unwrap();
    std::fs::write(
        product.join("AGENTS.md"),
        "# Navigation\n\n- [current-capabilities.md](./current-capabilities.md) - Current capabilities.\n",
    )
    .unwrap();
    git(&repo, &["add", "docs/koolade/product"]);
    git(
        &repo,
        &["commit", "-qm", "publish fixture product documentation"],
    );
    let base = git(&repo, &["rev-parse", "HEAD"]);
    std::fs::write(repo.join("implemented.txt"), "search queries persist\n").unwrap();
    git(&repo, &["add", "implemented.txt"]);
    git(&repo, &["commit", "-qm", "implement search"]);
    let merged = git(&repo, &["rev-parse", "HEAD"]);
    let store_root = repo.with_extension("private-state");
    let store = crate::artifacts::planning_store::PlanningStore::for_code_repository(
        uuid::Uuid::new_v4(),
        &store_root,
        &repo,
    );
    let mut state = PlannerState::load_with_store(&repo, &store).unwrap();
    state.bootstrap_missing().unwrap();
    // PlannerState resolves the pin from the final published code revision;
    // bind every subsequent local proposal to that exact snapshot.
    let store = state.planning_store.clone();
    let feature = crate::domain::ArtifactIdentity::preserve_markdown(
        "# CHG-001: Search\n\n**Status:** Implementing\n\n## Intent\n\nSave searches.\n\n## Current Behavior\n\nNo persistence.\n\n## Desired Behavior\n\nQueries persist.\n\n## Scope\n\nSearch.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nQueries persist.\n\n## Decisions and Assumptions\n\nUse local store.\n\n## Acceptance Criteria\n\nQuery survives restart.\n",
        None,
        "CHG-001",
        "Search",
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&feature)
        .unwrap()
        .unwrap();
    let feature = crate::domain::ChangeMetadata::write_markdown(
        &feature,
        &identity,
        crate::domain::ChangeStatus::Implementing,
    )
    .unwrap();
    let proposal = crate::core::apply::private_document_draft_changes(
        &store,
        &[("feature:CHG-001".to_owned(), feature.clone())],
    )
    .unwrap();
    let revision = store.revision().unwrap();
    store
        .transaction_with_revision(&proposal, Some(&revision))
        .unwrap();
    let mut workflow = workflow::Workflow::default();
    crate::artifacts::task_docs::save_workflow(&store, &workflow).unwrap();
    let feature = crate::core::lifecycle::test_support::approve_and_review_feature(
        &store,
        &mut workflow,
        "CHG-001",
        &feature,
    )
    .unwrap();
    std::fs::create_dir_all(store.root.join("planning/tasks/search")).unwrap();
    let ticket_text = "# Save query\n\nFeature ID: CHG-001\nRepository: root\n";
    let batch_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let ticket_markdown = crate::domain::ArtifactIdentity::preserve_markdown_with_parent(
        ticket_text,
        None,
        "TASK-001",
        "Save query",
        Some(&batch_uid),
    )
    .unwrap();
    let task_identity = crate::domain::ArtifactIdentity::from_markdown(&ticket_markdown)
        .unwrap()
        .unwrap();
    let task_metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&task_identity, "root", Vec::new()).unwrap();
    let ticket = crate::artifacts::task_docs::embed(&ticket_markdown, &task_metadata).unwrap();
    std::fs::write(
        store.root.join("planning/tasks/search/001-save-query.md"),
        &ticket,
    )
    .unwrap();
    let initial_state = PlannerState::load_with_store(&repo, &store).unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&feature)
        .unwrap()
        .unwrap();
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &initial_state.planning_store,
        "CHG-001",
        &identity,
        &feature,
        &workflow::feature_contract(&feature),
    )
    .unwrap();
    if let Some((path, bytes, check)) = crate::core::lifecycle::plan_snapshot::record_change(
        &initial_state.planning_store,
        &snapshot,
    )
    .unwrap()
    {
        let revision = initial_state.planning_store.revision().unwrap();
        initial_state
            .planning_store
            .transaction_with_revision_and_record_revisions(
                &[(path, bytes)],
                Some(&revision),
                &[check],
            )
            .unwrap();
    }
    let state = PlannerState::load_with_store(&repo, &store).unwrap();
    let task: crate::core::implementation::Implementation = serde_json::from_value(serde_json::json!({
            "ticket":"planning/tasks/search/001-save-query.md","ticket_text":crate::artifacts::task_docs::visible_content(&ticket),"branch":"koolade/task",
            "base":"main","base_commit":base,"task_repository":repo,"task_repository_kind":"clone","status":"completed","detail":"",
            "pr_url":null,"verified_head":merged,"merged_commit":merged
        })).unwrap();
    let contract = crate::core::contract_snapshot::BatchContract {
        feature_id: "CHG-001".into(),
        feature_specification: feature.clone(),
        product_modules: [(
            "current-capabilities".to_string(),
            std::fs::read_to_string(repo.join("docs/koolade/product/current-capabilities.md"))
                .unwrap(),
        )]
        .into(),
        repository_bases: Default::default(),
        configuration: String::new(),
    };
    let candidate = Candidate {
        feature_id: "CHG-001".into(),
        batch_directory: "planning/tasks/search".into(),
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
                "category":"General","feature_id":"CHG-001",
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
