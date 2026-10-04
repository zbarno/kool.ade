use super::*;

#[test]
fn merged_commit_is_inspected_before_product_truth_is_checkpointed() {
    let (repo, state, candidate, feature) = fixture();
    let merged = candidate.tasks[0].merged_commit.as_deref().unwrap();
    let updated_feature = feature.replace(
        "**Status:** Implementing",
        &format!("**Status:** Implemented\n\n**Implementation:** {merged}"),
    );
    let product = candidate.contract.product_modules["current-capabilities"].clone()
        + "\nSearch queries persist across restart in the merged implementation.\n";
    let response = serde_json::json!({"schema_version":2,"assistant_message":"Reconciled persisted search queries.",
            "document_updates":[{"document_id":"feature:CHG-001","content":updated_feature,"status":"implemented"},
                {"document_id":"product:current-capabilities","content":product}]}).to_string();
    let (progress, _events) = mpsc::channel();
    let (updated, _) = run(
        &state,
        &candidate,
        &StaticHarness(response),
        progress,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(updated.active_feature.is_none());
    assert!(
        std::fs::read_to_string(
            repo.join(".koolade-packet/planning/product/current-capabilities.md")
        )
        .unwrap()
        .contains("Search queries persist across restart")
    );
    assert!(
        std::fs::read_to_string(
            repo.join(".koolade-packet/planning/changes/CHG-001-search/specification.md")
        )
        .unwrap()
        .contains(merged)
    );
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]), "3");
    let _ = std::fs::remove_dir_all(repo);
}

/// Acts as a competing writer: while the model is "running" it edits the
/// active feature document on disk, so the snapshot the run validated
/// against is stale by the time the apply section is reached.
struct DriftHarness {
    envelope: String,
    feature_path: std::path::PathBuf,
}
impl AiHarness for DriftHarness {
    fn label(&self) -> String {
        "drift fixture".into()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok("fixture".into())
    }
    fn execute(
        &self,
        _request: &PlanningRequest,
    ) -> Result<HarnessOutcome, crate::error::AppError> {
        let text = std::fs::read_to_string(&self.feature_path)
            .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
        std::fs::write(
            &self.feature_path,
            format!("{text}\n\nExternal edit arrived.\n"),
        )
        .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
        Ok(HarnessOutcome {
            final_text: self.envelope.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

#[test]
fn drifting_project_is_deferred_not_clobbered() {
    let (repo, state, candidate, feature) = fixture();
    let merged = candidate.tasks[0].merged_commit.as_ref().unwrap().clone();
    let updated_feature = feature.replace(
        "**Status:** Implementing",
        &format!("**Status:** Implemented\n\n**Implementation:** {merged}"),
    );
    let product = candidate.contract.product_modules["current-capabilities"].clone()
        + "\nSearch queries persist across restart in the merged implementation.\n";
    let response = serde_json::json!({"schema_version":2,"assistant_message":"Reconciled persisted search queries.",
            "document_updates":[{"document_id":"feature:CHG-001","content":updated_feature,"status":"implemented"},
                {"document_id":"product:current-capabilities","content":product}]}).to_string();
    let feature_path =
        repo.join(".koolade-packet/planning/changes/CHG-001-search/specification.md");
    let harness = DriftHarness {
        envelope: response,
        feature_path: feature_path.clone(),
    };
    let (progress, _events) = mpsc::channel();
    let error = run_with_settle_window(
        &state,
        &candidate,
        &harness,
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
    // Reconciliation must not have checkpointed or applied anything.
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]), "2");
    assert!(
        !std::fs::read_to_string(
            repo.join(".koolade-packet/planning/product/current-capabilities.md")
        )
        .unwrap()
        .contains("persist across restart")
    );
    // The external edit survives untouched by the deferral.
    assert!(
        std::fs::read_to_string(feature_path)
            .unwrap()
            .contains("External edit arrived.")
    );
    let _ = std::fs::remove_dir_all(repo);
}

#[test]
fn candidate_waits_for_every_task_to_reach_merged_state() {
    let (repo, _, expected, _) = fixture();
    std::fs::write(
        repo.join(".koolade-packet/planning/tasks/search/contract.json"),
        serde_json::to_string_pretty(&expected.contract).unwrap(),
    )
    .unwrap();
    let mut workflow = workflow::Workflow::default();
    workflow.task_batches.push(workflow::TaskBatchRef {
        identity: None,
        feature: "Search".into(),
        directory: ".koolade-packet/planning/tasks/search".into(),
        count: 1,
    });
    crate::artifacts::task_docs::save_workflow(&repo, &workflow).unwrap();
    let storage = crate::core::implementation::state_dir(&repo, &expected.tasks[0].ticket).unwrap();
    std::fs::create_dir_all(&storage).unwrap();
    let mut record = expected.tasks[0].clone();
    record.status = ImplementationStatus::AwaitingReview;
    std::fs::write(
        storage.join("state.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let state = PlannerState::load(&repo).unwrap();
    assert!(candidate(&state).unwrap().is_none());
    record.status = ImplementationStatus::Completed;
    std::fs::write(
        storage.join("state.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let selected = candidate(&state).unwrap().unwrap();
    assert_eq!(selected.tasks.len(), 1);
    assert_eq!(selected.feature_id, "CHG-001");
    let _ = std::fs::remove_dir_all(repo);
}

#[test]
fn conflicting_merged_behavior_creates_review_card_without_rewriting_product() {
    let (repo, state, candidate, feature) = fixture();
    let before =
        std::fs::read(repo.join(".koolade-packet/planning/product/current-capabilities.md"))
            .unwrap();
    let response = serde_json::json!({"schema_version":2,"assistant_message":"Merged code differs from approved intent.",
            "document_updates":[{"document_id":"feature:CHG-001","content":feature.replace("**Status:** Implementing", "**Status:** Reconciliation"),"status":"reconciliation"}],
            "open_items_added":[{"kind":"Assumption","priority":"Normal","authority":"Review",
                "category":"General","assigned_to":"All","feature_id":"CHG-001",
                "question":"Reconciliation found a material mismatch in saved-query retention; approve a corrective task?",
                "reason":"Merged behavior omits the approved retention rule.",
                "recommendation":"Implement the approved retention rule before changing product truth.",
                "evidence":"Merged implementation commit omits the retention code path."}]}).to_string();
    let (progress, _events) = mpsc::channel();
    let (updated, _) = run(
        &state,
        &candidate,
        &StaticHarness(response),
        progress,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(repo.join(".koolade-packet/planning/product/current-capabilities.md"))
            .unwrap(),
        before
    );
    assert_eq!(updated.items.len(), 1);
    assert_eq!(updated.items[0].authority, Authority::Review);
    assert!(
        updated
            .active_feature
            .as_ref()
            .unwrap()
            .1
            .contains("**Status:** Reconciliation")
    );
    let _ = std::fs::remove_dir_all(repo);
}
