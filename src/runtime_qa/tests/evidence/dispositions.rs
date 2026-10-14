use super::*;

#[test]
fn manager_dispositions_are_immutable_idempotent_and_loaded_with_their_run() {
    let mut fixture = report_fixture();
    let finding = QaFindingV1 {
        finding_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        code_sha: fixture.request.code_head_sha.clone(),
        scenario_uid: fixture.scenarios[0].id.clone(),
        requirement_uids: fixture.request.requirement_ids.clone(),
        category: QaFindingCategory::Usability,
        severity: QaFindingSeverity::Low,
        expected: "The main view renders".into(),
        actual: "The main view renders with a minor label issue".into(),
        reproducible_steps: vec!["Open the page".into()],
        screenshot_ids: vec![fixture.screenshot.id.clone()],
        trace_ids: vec!["0".into()],
        smallest_fix: "Clarify the label".into(),
        blocking: false,
    };
    fixture.result.findings.push(finding.clone());
    let directory = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(directory.path());
    store
        .save_run_with_screenshots(
            &fixture.request,
            &fixture.scenarios,
            &fixture.evidence,
            &fixture.result,
            &[fixture.screenshot],
        )
        .unwrap();
    let disposition = QaFindingDispositionV1 {
        schema_version: 1,
        finding_id: finding.finding_uid,
        qa_run_id: fixture.request.qa_run_id.clone(),
        parent_task_id: fixture.request.parent_task_uid.clone(),
        code_sha: fixture.request.code_head_sha.clone(),
        kind: QaFindingDispositionKind::AskUser,
        status: QaFindingDispositionStatus::AwaitingAnswer,
        reason: "Confirm whether to address this nonblocking issue".into(),
        owner_item_uid: None,
        impacted_requirements: fixture.request.requirement_ids.clone(),
        decision_id: None,
        created_revision: 1,
        expected_revision: 1,
    };

    store.save_disposition(&disposition).unwrap();
    store.save_disposition(&disposition).unwrap();
    assert_eq!(
        store.load_dispositions(&fixture.request.qa_run_id).unwrap(),
        std::slice::from_ref(&disposition)
    );

    let mut changed = disposition;
    changed.reason = "Different decision".into();
    assert!(store.save_disposition(&changed).is_err());
}

#[test]
fn screenshot_checkpoint_can_be_bound_to_the_interaction_that_captured_it() {
    let mut fixture = report_fixture();
    fixture.evidence.actions[1].command = "click".into();
    fixture.evidence.actions[1].pre_state = Some("before click".into());
    fixture.result.evidence_manifest_sha256 = fixture.evidence.sha256().unwrap();
    validate_report(
        &fixture.request,
        &fixture.scenarios,
        &fixture.evidence,
        &fixture.result,
    )
    .unwrap();
}
