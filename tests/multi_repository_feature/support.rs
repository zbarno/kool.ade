use super::*;

pub(super) fn approve_and_record_reviewer_receipt(
    state: &PlannerState,
    workflow: &mut Workflow,
    feature_id: &str,
    feature: &str,
) {
    use koolade::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render};
    use sha2::Digest;

    let store = &state.planning_store;
    let identity = koolade::domain::ArtifactIdentity::from_markdown(feature)
        .unwrap()
        .unwrap();
    let contract = koolade::core::workflow::feature_contract(feature);
    let author_run_id = "00000000-0000-4000-8000-000000000001";
    let author_session_id = "00000000-0000-4000-8000-000000000002";
    let author = serde_json::json!({
        "schemaVersion": 1,
        "uid": identity.uid.clone(),
        "createdAtMs": 1,
        "updatedAtMs": 1,
        "revision": 1,
        "data": {
            "featureUid": identity.uid.clone(),
            "approvalContractSha256": format!("{:x}", sha2::Sha256::digest(contract.as_bytes())),
            "authorRunId": author_run_id,
            "authorSessionId": author_session_id
        }
    });
    store
        .transaction_with_record_revisions(
            &[(
                format!("state/lifecycle_plan_authors/{}.json", identity.uid),
                serde_json::to_vec_pretty(&author).unwrap(),
            )],
            &[koolade::artifacts::planning_store::RecordRevisionCheck {
                path: format!("state/lifecycle_plan_authors/{}.json", identity.uid),
                expected_revision: 0,
            }],
        )
        .unwrap();
    koolade::core::workflow::approve_feature(store, workflow, feature_id).unwrap();

    let snapshot_path = store
        .list_record_paths("state/lifecycle_plans")
        .unwrap()
        .into_iter()
        .find(|path| path.ends_with(&format!("{}.json", identity.uid)))
        .or_else(|| {
            store
                .list_record_paths("state/lifecycle_plans")
                .unwrap()
                .into_iter()
                .next()
        })
        .expect("feature confirmation persists a plan snapshot");
    let (record, _) = store
        .read_record::<serde_json::Value>(&snapshot_path)
        .unwrap();
    let snapshot = &record["data"];
    let reviewer_run_id = "00000000-0000-4000-8000-000000000003";
    let reviewer_session_id = "00000000-0000-4000-8000-000000000004";
    let manifest = render(&RoleInputV1 {
        role: PromptRole::PlanReviewer,
        workspace_uid: store.project_id.hyphenated().to_string(),
        feature_uid: identity.uid.clone(),
        task_uid: None,
        original_goal: snapshot["intent"].as_str().unwrap().to_owned(),
        approved_spec_ref: Some(snapshot["uid"].as_str().unwrap().to_owned()),
        approved_spec_hash: Some(snapshot["contentHash"].as_str().unwrap().to_owned()),
        decision_refs: Vec::new(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "plan-review/test-only/v1".into(),
        output_schema_version: PromptRole::PlanReviewer.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: snapshot["specRevision"].as_str().unwrap().to_owned(),
            subject_hash: snapshot["contentHash"].as_str().unwrap().to_owned(),
            author_run_id: author_run_id.into(),
            reviewer_run_id: reviewer_run_id.into(),
            author_session_id: Some(author_session_id.into()),
            reviewer_session_id: Some(reviewer_session_id.into()),
            requirements_matrix: snapshot["requirements"]
                .as_array()
                .unwrap()
                .iter()
                .map(|requirement| requirement["text"].as_str().unwrap().to_owned())
                .collect(),
            explicit_exclusions: snapshot["nonGoals"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_owned())
                .collect(),
        },
    })
    .unwrap()
    .manifest;
    let receipt = koolade::core::lifecycle::GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: snapshot["uid"].as_str().unwrap().to_owned(),
        subject_sha256: snapshot["contentHash"].as_str().unwrap().to_owned(),
        actor: koolade::core::lifecycle::GateActor::IndependentReviewer,
        decision: koolade::core::lifecycle::GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec![format!("plan:{}", snapshot["uid"].as_str().unwrap())],
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some(reviewer_run_id.into()),
        author_run_id: Some(author_run_id.into()),
        reviewer_session_id: Some(reviewer_session_id.into()),
        author_session_id: Some(author_session_id.into()),
    };
    let revision = store.revision().unwrap();
    let verdict = koolade::harness::prompt_catalog::ReviewVerdictV1 {
        subject_hash: snapshot["contentHash"].as_str().unwrap().to_owned(),
        pass: true,
        outcome: Some(koolade::harness::prompt_catalog::ReviewOutcomeV1::Pass),
        coverage: snapshot["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|requirement| koolade::harness::prompt_catalog::CoverageV1 {
                requirement: requirement["stableId"].as_str().unwrap().to_owned(),
                status: koolade::harness::prompt_catalog::CoverageStatus::Pass,
                evidence: vec![format!("plan:{}", snapshot["uid"].as_str().unwrap())],
            })
            .collect(),
        findings: Vec::new(),
        untested: Vec::new(),
    };
    koolade::core::workflow::record_plan_review_if_current(
        store, feature_id, &receipt, &verdict, &revision,
    )
    .unwrap();
    *workflow = koolade::artifacts::task_docs::load_workflow(store).unwrap();
}
