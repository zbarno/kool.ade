use super::*;
use crate::{
    domain::{ItemKind, OpenItem, Priority},
    harness::HarnessOutcome,
};
use std::path::PathBuf;
struct StaticHarness(String);
impl AiHarness for StaticHarness {
    fn label(&self) -> String {
        "investigation fixture".into()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok("fixture".into())
    }
    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, crate::error::AppError> {
        assert_eq!(request.mode, ExecutionMode::Investigation);
        assert_eq!(request.reasoning_level, "off");
        let workspace = request
            .execution_workspace
            .as_ref()
            .expect("investigation receives pinned read-only tools");
        workspace.validate().unwrap();
        assert_eq!(request.task_id.as_deref(), Some(workspace.task_id.as_str()));
        assert!(
            request
                .prompt_body
                .contains("AUTONOMOUS AGENT ITEM CLR-001")
        );
        assert!(
            request
                .prompt_body
                .contains("no more than six read-only tool calls")
        );
        assert!(
            request
                .prompt_body
                .contains("Do not include requested_action")
        );
        assert!(request.prompt_body.contains("\"open_items_updated\":["));
        Ok(HarnessOutcome {
            final_text: self.0.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}
fn fixture() -> (PathBuf, PathBuf, PlannerState, String) {
    let root = std::env::temp_dir().join(format!(
        "koolade_agent_item_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        ["init", "-q"].as_slice(),
        ["config", "user.name", "Fixture"].as_slice(),
        ["config", "user.email", "fixture@example.test"].as_slice(),
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
    }
    std::fs::write(root.join("README.md"), "Synthetic source snapshot\n").unwrap();
    for args in [
        ["add", "README.md"].as_slice(),
        ["commit", "-qm", "Synthetic source baseline"].as_slice(),
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
    }
    crate::core::state::test_support::seed_published_product_document(&root).unwrap();
    let workspace_id = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        std::fs::canonicalize(&root)
            .unwrap()
            .to_string_lossy()
            .as_bytes(),
    );
    let local_root = root.with_file_name(format!(
        "{}-local",
        root.file_name().unwrap().to_string_lossy()
    ));
    let store = crate::artifacts::planning_store::PlanningStore::for_code_repository(
        workspace_id,
        &local_root,
        &root,
    );
    let feature = crate::test_support::current_change_fixture(
        "# CHG-001: Search\n\n**Status:** Draft\n\n## Intent\n\nImprove search.\n\n## Current Behavior\n\nQuery persistence is unknown.\n\n## Desired Behavior\n\nQueries persist.\n\n## Scope\n\nSearch.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nQueries persist.\n\n## Decisions and Assumptions\n\nNone.\n\n## Acceptance Criteria\n\nRestart retains query.\n",
        crate::domain::ChangeStatus::Draft,
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&feature)
        .unwrap()
        .unwrap();
    let feature_dir = root.join("docs/koolade/features");
    let feature_path = feature_dir.join(format!("{}.md", identity.uid));
    std::fs::write(&feature_path, &feature).unwrap();
    std::fs::write(
        feature_dir.join("AGENTS.md"),
        format!(
            "# Navigation\n\n- [CHG-001: Search](./{}.md) - Synthetic published feature.\n",
            identity.uid
        ),
    )
    .unwrap();
    for args in [
        ["add", "docs/koolade/features"].as_slice(),
        ["commit", "-qm", "Seed synthetic published feature"].as_slice(),
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
    }
    let mut item = OpenItem::new(
        "CLR-001".into(),
        Priority::Blocking,
        ItemKind::Ambiguity,
        "General".into(),
        "Does current query persistence survive restart?".into(),
        "Repository evidence may answer this.".into(),
    );
    item.authority = Authority::Agent;
    item.feature_id = Some("CHG-001".into());
    let state = PlannerState::load_with_store(&root, &store).unwrap();
    let (changes, checks) =
        crate::artifacts::items_io::record_changes(&state.planning_store, &[item], &[]).unwrap();
    state
        .planning_store
        .transaction_with_record_revisions(&changes, &checks)
        .unwrap();
    let state = PlannerState::load_with_store(&root, &store).unwrap();
    (root, local_root, state, feature)
}
#[test]
fn evidence_resolves_agent_item_without_chat_question() {
    let (root, local_root, state, feature) = fixture();
    let revised = feature.replace("Query persistence is unknown.",
        "Query persistence is not implemented; src/search.rs only keeps the active query in memory.");
    let response = serde_json::json!({"schema_version":2,"assistant_message":"Verified current behavior from src/search.rs.",
        "document_updates":[{"document_id":"feature:CHG-001","content":revised}],
        "open_items_resolved":["CLR-001"],"next_question_id":null}).to_string();
    let (progress, _events) = mpsc::channel();
    let (updated, _) = run(
        &state,
        "CLR-001",
        &StaticHarness(response),
        progress,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(updated.items.is_empty());
    assert!(
        updated
            .active_feature
            .as_ref()
            .unwrap()
            .1
            .contains("src/search.rs")
    );
    let draft = crate::core::apply::load_current(&updated.planning_store)
        .unwrap()
        .expect("investigation stores a private specification proposal");
    assert!(draft.changes.iter().any(|change| {
        change.document_id == "feature:CHG-001" && change.content.contains("src/search.rs")
    }));
    let identity = crate::domain::ArtifactIdentity::from_markdown(&feature)
        .unwrap()
        .unwrap();
    let pin = state.planning_store.published_spec_pin.as_ref().unwrap();
    let published = crate::core::published_specs::PublishedSpecReader::new(
        &root,
        pin.repository_id.clone(),
        pin.reference.clone(),
    )
    .unwrap()
    .read(pin, &format!("docs/koolade/features/{}.md", identity.uid))
    .unwrap();
    assert_eq!(published.content, feature);
    assert!(
        std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&root)
            .output()
            .unwrap()
            .stdout
            .is_empty()
    );
    let (open_items, resolved_items, _) =
        crate::artifacts::items_io::load_store(&updated.planning_store).unwrap();
    assert!(open_items.is_empty());
    assert_eq!(resolved_items[0].id, "CLR-001");
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(local_root);
}
/// Acts as a competing writer: updates the durable question while the
/// model is running, as another gated writer would.
struct DriftingPeer {
    raw: String,
    store: crate::artifacts::planning_store::PlanningStore,
}
impl AiHarness for DriftingPeer {
    fn label(&self) -> String {
        "drifting-peer".into()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok("fixture".into())
    }
    fn execute(&self, _req: &PlanningRequest) -> Result<HarnessOutcome, crate::error::AppError> {
        let (mut open, resolved, _) = crate::artifacts::items_io::load_store(&self.store)
            .map_err(|error| crate::error::AppError::Other(error.to_string()))?;
        let item = open
            .iter_mut()
            .find(|item| item.id == "CLR-001")
            .ok_or_else(|| crate::error::AppError::Other("peer question disappeared".into()))?;
        item.recommendation = "Concurrent peer recommendation".into();
        let (changes, checks) =
            crate::artifacts::items_io::record_changes(&self.store, &open, &resolved)
                .map_err(|error| crate::error::AppError::Other(error.to_string()))?;
        self.store
            .transaction_with_record_revisions(&changes, &checks)
            .map_err(|error| crate::error::AppError::Other(error.to_string()))?;
        Ok(HarnessOutcome {
            final_text: self.raw.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

#[test]
fn drifting_project_defers_instead_of_clobbering() {
    let (root, local_root, state, _) = fixture();
    let response = serde_json::json!({"schema_version":2,"assistant_message":"Cannot settle autonomously.",
        "document_updates":[],"open_items_updated":[{"id":"CLR-001","authority":"Review",
            "recommendation":"Defer.","evidence":"Insufficient evidence."}],"next_question_id":null}).to_string();
    let (progress, _events) = mpsc::channel();
    let error = run_with_settle_window(
        &state,
        "CLR-001",
        &DriftingPeer {
            raw: response,
            store: state.planning_store.clone(),
        },
        progress,
        Arc::new(AtomicBool::new(false)),
        std::time::Duration::from_millis(200),
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message.starts_with(DEFER_PREFIX),
        "expected a benign deferral, got: {message}"
    );
    // The peer's item update survives; the investigation wrote nothing.
    assert!(
        state
            .active_feature
            .as_ref()
            .unwrap()
            .1
            .contains("Query persistence is unknown.")
    );
    let current = PlannerState::load_with_store(&root, &state.planning_store).unwrap();
    let current_item = current
        .items
        .iter()
        .find(|item| item.id == "CLR-001")
        .unwrap();
    assert_eq!(
        current_item.recommendation,
        "Concurrent peer recommendation"
    );
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(local_root);
}

#[test]
fn uncertain_agent_item_becomes_review_with_evidence() {
    let (root, local_root, state, _) = fixture();
    let response = serde_json::json!({"schema_version":2,"assistant_message":"Repository evidence cannot establish retention policy.",
        "document_updates":[],"open_items_updated":[{"id":"CLR-001","authority":"Review",
            "recommendation":"Provisionally retain only the last ten queries.",
            "evidence":"src/search.rs has no persistence or retention contract."}],"next_question_id":null}).to_string();
    let (progress, _events) = mpsc::channel();
    let (updated, _) = run(
        &state,
        "CLR-001",
        &StaticHarness(response),
        progress,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(updated.items[0].authority, Authority::Review);
    assert!(!updated.items[0].recommendation.is_empty());
    assert!(!updated.items[0].evidence.is_empty());
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(local_root);
}
