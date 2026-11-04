use super::*;

#[test]
fn merged_commit_is_inspected_before_product_truth_is_checkpointed() {
    let (repo, state, candidate, feature) = fixture();
    let code_head = git(&repo, &["rev-parse", "HEAD"]);
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
    let product = crate::artifacts::product_docs::current_document(
        &updated.planning_store,
        "product:current-capabilities",
    )
    .unwrap()
    .unwrap();
    assert!(product.contains("Search queries persist across restart"));
    let feature = crate::artifacts::product_docs::current_document(
        &updated.planning_store,
        "feature:CHG-001",
    )
    .unwrap()
    .unwrap();
    assert!(feature.contains(merged));
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), code_head);
    assert!(!repo.join("planning").exists());
    let _ = std::fs::remove_dir_all(repo);
}

/// Acts as a competing writer: while the model is "running" it edits the
/// active feature document on disk, so the snapshot the run validated
/// against is stale by the time the apply section is reached.
struct DriftHarness {
    envelope: String,
    store: crate::artifacts::planning_store::PlanningStore,
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
        let current = crate::core::apply::load_current(&self.store)
            .map_err(|error| crate::error::AppError::Other(error.to_string()))?
            .ok_or_else(|| crate::error::AppError::Other("fixture proposal is missing".into()))?;
        let feature = current
            .changes
            .iter()
            .find(|change| change.document_id == "feature:CHG-001")
            .ok_or_else(|| crate::error::AppError::Other("fixture feature is missing".into()))?
            .content
            .clone();
        let update = format!("{feature}\n\nExternal edit arrived.\n");
        let changes = crate::core::apply::private_document_draft_changes(
            &self.store,
            &[("feature:CHG-001".to_owned(), update)],
        )
        .map_err(|error| crate::error::AppError::Other(error.to_string()))?;
        let revision = self
            .store
            .revision()
            .map_err(|error| crate::error::AppError::Other(error.to_string()))?;
        self.store
            .transaction_with_revision(&changes, Some(&revision))
            .map_err(|error| crate::error::AppError::Other(error.to_string()))?;
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
    let code_head = git(&repo, &["rev-parse", "HEAD"]);
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
    let harness = DriftHarness {
        envelope: response,
        store: state.planning_store.clone(),
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
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), code_head);
    assert!(
        !crate::artifacts::product_docs::render_product(&state.planning_store)
            .unwrap()
            .unwrap()
            .contains("persist across restart")
    );
    // The external edit survives untouched by the deferral.
    assert!(
        crate::artifacts::product_docs::current_document(&state.planning_store, "feature:CHG-001",)
            .unwrap()
            .unwrap()
            .contains("External edit arrived.")
    );
    let _ = std::fs::remove_dir_all(repo);
}

#[test]
fn candidate_waits_for_every_task_to_reach_merged_state() {
    let (repo, fixture_state, expected, _) = fixture();
    let mut state = PlannerState::load_with_store(&repo, &fixture_state.planning_store).unwrap();
    let contract_path = state
        .planning_store
        .root
        .join("planning/tasks/search/contract.json");
    std::fs::create_dir_all(contract_path.parent().unwrap()).unwrap();
    std::fs::write(
        contract_path,
        serde_json::to_string_pretty(&expected.contract).unwrap(),
    )
    .unwrap();
    let mut workflow = crate::artifacts::task_docs::load_workflow(&state.planning_store).unwrap();
    workflow.task_batches.push(workflow::TaskBatchRef {
        identity: None,
        feature: "Search".into(),
        directory: "planning/tasks/search".into(),
        count: 1,
        created_at_ms: 0,
    });
    crate::artifacts::task_docs::save_workflow(&state.planning_store, &workflow).unwrap();
    state = PlannerState::load_with_store(&repo, &state.planning_store).unwrap();
    let storage = crate::core::implementation::state_dir(
        &state.planning_store.root,
        &expected.tasks[0].ticket,
    )
    .unwrap();
    std::fs::create_dir_all(&storage).unwrap();
    let mut record = expected.tasks[0].clone();
    record.status = ImplementationStatus::AwaitingReview;
    std::fs::write(
        storage.join("state.json"),
        crate::core::implementation::serialize_state(&record).unwrap(),
    )
    .unwrap();
    assert!(candidate(&state).unwrap().is_none());
    record.status = ImplementationStatus::Completed;
    std::fs::write(
        storage.join("state.json"),
        crate::core::implementation::serialize_state(&record).unwrap(),
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
    let before = crate::artifacts::product_docs::render_product(&state.planning_store)
        .unwrap()
        .unwrap();
    let response = serde_json::json!({"schema_version":2,"assistant_message":"Merged code differs from approved intent.",
            "document_updates":[{"document_id":"feature:CHG-001","content":feature.replace("**Status:** Implementing", "**Status:** Reconciliation"),"status":"reconciliation"}],
            "open_items_added":[{"kind":"Assumption","priority":"Normal","authority":"Review",
                "category":"General","feature_id":"CHG-001",
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
        crate::artifacts::product_docs::render_product(&state.planning_store)
            .unwrap()
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
