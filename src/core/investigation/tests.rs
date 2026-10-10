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
fn fixture() -> (PathBuf, PlannerState, String) {
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
    let legacy = crate::artifacts::spec_doc::bootstrap_template("Demo");
    std::fs::create_dir_all(root.join("planning")).unwrap();
    std::fs::write(root.join("planning/specification.md"), &legacy).unwrap();
    crate::artifacts::product_docs::migrate(&root, &legacy).unwrap();
    std::fs::create_dir_all(root.join(".koolade-packet/planning")).unwrap();
    let feature = "# CHG-001: Search\n\n**Status:** Draft\n\n## Intent\n\nImprove search.\n\n## Current Behavior\n\nQuery persistence is unknown.\n\n## Desired Behavior\n\nQueries persist.\n\n## Scope\n\nSearch.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nQueries persist.\n\n## Decisions and Assumptions\n\nNone.\n\n## Acceptance Criteria\n\nRestart retains query.\n".to_string();
    let feature_path =
        root.join(".koolade-packet/planning/changes/CHG-001-search/specification.md");
    std::fs::create_dir_all(feature_path.parent().unwrap()).unwrap();
    std::fs::write(&feature_path, &feature).unwrap();
    let mut item = OpenItem::new(
        "CLR-001".into(),
        Priority::Blocking,
        ItemKind::Ambiguity,
        "General".into(),
        Some("All".into()),
        "Does current query persistence survive restart?".into(),
        "Repository evidence may answer this.".into(),
    );
    item.authority = Authority::Agent;
    item.feature_id = Some("CHG-001".into());
    std::fs::write(
        root.join(".koolade-packet/planning/open-items.md"),
        crate::artifacts::items_io::serialize(&[item]),
    )
    .unwrap();
    let state = PlannerState::load(&root).unwrap();
    (root, state, feature)
}
#[test]
fn evidence_resolves_agent_item_without_chat_question() {
    let (root, state, feature) = fixture();
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
    let (open_items, resolved_items, _) =
        crate::artifacts::items_io::load_store(&updated.planning_store).unwrap();
    assert!(open_items.is_empty());
    assert_eq!(resolved_items[0].id, "CLR-001");
    let _ = std::fs::remove_dir_all(root);
}
/// Acts as a competing writer: edits the active feature document while
/// the model is "running", as another gated writer would.
struct DriftingPeer {
    raw: String,
    root: PathBuf,
}
impl AiHarness for DriftingPeer {
    fn label(&self) -> String {
        "drifting-peer".into()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok("fixture".into())
    }
    fn execute(&self, _req: &PlanningRequest) -> Result<HarnessOutcome, crate::error::AppError> {
        let path = self
            .root
            .join(".koolade-packet/planning/changes/CHG-001-search/specification.md");
        let text = std::fs::read_to_string(&path)
            .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
        std::fs::write(
            &path,
            format!("{text}\nConcurrent edit by a gated writer.\n"),
        )
        .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
        Ok(HarnessOutcome {
            final_text: self.raw.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

#[test]
fn drifting_project_defers_instead_of_clobbering() {
    let (root, state, _) = fixture();
    let response = serde_json::json!({"schema_version":2,"assistant_message":"Cannot settle autonomously.",
        "document_updates":[],"open_items_updated":[{"id":"CLR-001","authority":"Review",
            "recommendation":"Defer.","evidence":"Insufficient evidence."}],"next_question_id":null}).to_string();
    let (progress, _events) = mpsc::channel();
    let error = run_with_settle_window(
        &state,
        "CLR-001",
        &DriftingPeer {
            raw: response,
            root: root.clone(),
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
    // The peer's edit survives; the investigation wrote nothing.
    let feature_now = std::fs::read_to_string(
        root.join(".koolade-packet/planning/changes/CHG-001-search/specification.md"),
    )
    .unwrap();
    assert!(feature_now.contains("Concurrent edit by a gated writer."));
    assert!(feature_now.contains("Query persistence is unknown."));
    let items = crate::artifacts::items_io::parse(
        &std::fs::read_to_string(root.join(".koolade-packet/planning/open-items.md")).unwrap(),
    )
    .unwrap();
    assert!(items.iter().any(|item| item.id == "CLR-001"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn uncertain_agent_item_becomes_review_with_evidence() {
    let (root, state, _) = fixture();
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
}
