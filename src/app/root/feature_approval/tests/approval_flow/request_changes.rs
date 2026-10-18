use super::*;

#[test]
fn request_changes_is_candidate_bound_and_gives_the_exact_feature_work_all_review_evidence() {
    let _shield = crate::core::gitops::test_support::shield("feature-plan-remediation");
    let (mut app, root, _) = setup();
    if let Screen::Connected(project) = &mut app.screen {
        let store = &project.state.planning_store;
        let (changes, checks, _) = crate::artifacts::items_io::record_changes(
            store,
            &project.state.items,
            &project.state.resolved_items,
        )
        .unwrap();
        store
            .transaction_with_record_revisions(&changes, &checks)
            .unwrap();
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
    }
    let Screen::Connected(project) = &mut app.screen else {
        panic!("project remains connected")
    };
    let (_, markdown) = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == "CHG-004")
        .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .unwrap()
        .unwrap();
    let feature_work_key = "feature-work:CHG-004".to_owned();
    let mut feature_work = crate::core::planning_work::Work::new(
        feature_work_key.clone(),
        "Plan saved searches".into(),
        "Keep the saved search available after restart.".into(),
        "Feature planning work".into(),
    );
    feature_work.feature_id = Some("CHG-004".into());
    feature_work.feature_uid = Some(identity.uid);
    project.planning_work.push(feature_work);
    project.save_planning_work().unwrap();
    let (_, markdown) = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == "CHG-004")
        .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .unwrap()
        .unwrap();
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        "CHG-004",
        &identity,
        markdown,
        &crate::core::workflow::feature_contract(markdown),
    )
    .unwrap();

    app.approve_feature_only("CHG-004");
    record_test_plan_review(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::ChangesRequested,
    );
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected")
    };
    let persisted = crate::core::lifecycle::plan_snapshot::plan_review_verdict(
        &project.state.planning_store,
        &snapshot,
    )
    .unwrap()
    .unwrap();
    assert_eq!(persisted.findings[0].id, "F-TEST");
    let receipt = crate::core::lifecycle::plan_snapshot::plan_review_receipt(
        &project.state.planning_store,
        &snapshot,
    )
    .unwrap()
    .unwrap();
    let (_, current_spec) = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == "CHG-004")
        .unwrap();
    let updated_spec = current_spec.replace(
        "Persist saved searches and restore them after restarting.",
        "Persist saved searches and restore them after restarting with a clear completion result.",
    );
    let remediation = crate::core::lifecycle::PlanRemediationV1 {
        schema_version: 1,
        feature_uid: snapshot.feature_uid.clone(),
        snapshot_uid: snapshot.uid.clone(),
        snapshot_hash: snapshot.content_hash.clone(),
        review_receipt_uid: receipt.receipt_uid,
        dispositions: vec![crate::core::lifecycle::PlanFindingDispositionV1 {
            finding_id: "F-TEST".into(),
            action: crate::core::lifecycle::PlanFindingActionV1::RepairPlan {
                rationale: "Makes the success result observable.".into(),
            },
        }],
    };
    let reply = serde_json::to_string(&serde_json::json!({
        "schema_version": 2,
        "assistant_message": "I will clarify the success result.",
        "document_updates": [{
            "document_id": "feature:CHG-004",
            "content": updated_spec,
            "status": "ready"
        }],
        "open_items_added": [],
        "open_items_updated": [],
        "open_items_resolved": [],
        "planRemediation": remediation
    }))
    .unwrap();
    let no_op_reply = serde_json::to_string(&serde_json::json!({
        "schema_version": 2,
        "assistant_message": "I will resolve the review finding.",
        "document_updates": [{
            "document_id": "feature:CHG-004",
            "content": current_spec,
            "status": "ready"
        }],
        "open_items_added": [],
        "open_items_updated": [],
        "open_items_resolved": [],
        "planRemediation": remediation.clone()
    }))
    .unwrap();
    let (fake, calls) = harness(vec![
        r#"{"schema_version":2,"assistant_message":"I will resolve the review finding.","document_updates":[],"open_items_added":[],"open_items_updated":[],"open_items_resolved":[],"next_question_id":null}"#.into(),
    ]);
    app.task_harness = Some(fake);

    // Production boundary: exercise persisted-review routing, stale candidate
    // rejection, exact work identity, validated typed remediation, and its
    // atomic planning-store write. A widget or schema unit test cannot prove
    // this application path; it uses a temporary local Git/planning-store
    // fixture, no external service, and low setup cost.
    app.request_feature_changes("CHG-004", "stale-candidate-hash");
    assert!(matches!(&app.screen, Screen::Connected(project) if project.active_turn.is_none()));
    app.request_feature_changes("CHG-004", &snapshot.content_hash);
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected")
    };
    assert!(project.active_turn.is_some());
    assert_eq!(
        project.active_planning_work.as_deref(),
        Some(feature_work_key.as_str())
    );
    let started = Instant::now();
    while calls.lock().unwrap().is_empty() {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "Manager call did not start"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let manager_prompt = calls.lock().unwrap()[0].clone();
    assert!(
        manager_prompt.contains("F-TEST"),
        "Manager prompt omitted persisted review findings"
    );
    assert!(manager_prompt.contains("coverage"));
    assert!(manager_prompt.contains("review_receipt_uid"));
    assert!(manager_prompt.contains("requirement:"));

    assert!(
        !finish(&mut app),
        "a Request Changes response without typed dispositions must be rejected"
    );
    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected")
    };
    assert!(project.active_turn.is_none());
    assert!(
        project
            .state
            .planning_store
            .read(format!(
                "state/lifecycle_plan_remediations/{}.json",
                snapshot.uid
            ))
            .is_err()
    );

    let (no_op_harness, no_op_calls) = harness(vec![no_op_reply]);
    app.task_harness = Some(no_op_harness);
    app.request_feature_changes("CHG-004", &snapshot.content_hash);
    let started = Instant::now();
    while no_op_calls.lock().unwrap().is_empty() {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "no-op Manager retry did not start"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(no_op_calls.lock().unwrap()[0].contains("F-TEST"));
    assert!(
        !finish(&mut app),
        "RepairPlan must make a real change to the feature candidate"
    );

    let (retry_harness, retry_calls) = harness(vec![reply]);
    app.task_harness = Some(retry_harness);
    app.request_feature_changes("CHG-004", &snapshot.content_hash);
    let started = Instant::now();
    while retry_calls.lock().unwrap().is_empty() {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "valid Manager retry did not start"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(retry_calls.lock().unwrap()[0].contains("F-TEST"));
    assert!(
        finish(&mut app),
        "the typed Request Changes turn should complete"
    );

    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected")
    };
    let artifact_path = format!("state/lifecycle_plan_remediations/{}.json", snapshot.uid);
    let persisted = project.state.planning_store.read(&artifact_path).unwrap();
    let persisted: crate::core::lifecycle::PlanRemediationV1 =
        serde_json::from_slice(&persisted).unwrap();
    assert_eq!(persisted.snapshot_hash, snapshot.content_hash);
    assert_eq!(persisted.dispositions[0].finding_id, "F-TEST");
    assert!(!crate::core::workflow::feature_approved(
        &project.state.planning_store,
        &project.state.workflow,
        "CHG-004"
    ));
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
