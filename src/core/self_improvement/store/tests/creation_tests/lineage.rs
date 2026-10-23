use super::*;

fn linked_proposal(parent_feature_uid: &str) -> OpportunityV1 {
    let mut proposal = opportunity();
    proposal
        .related_feature_uids
        .push(parent_feature_uid.into());
    proposal.dedup_fingerprint = format!("{:0<64}", uuid::Uuid::new_v4().simple());
    proposal
}

#[test]
fn pursued_followups_record_lineage_and_stop_at_the_chain_depth_limit() {
    let temp = TempDir::new();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.0.join("planning"),
        StoreMode::ManagedLocal,
    );
    let opportunities = OpportunityStore::new(&store);
    let defaults = opportunities.discovery_settings().unwrap();
    opportunities
        .save_discovery_settings(
            &crate::core::opportunity::OpportunityDiscoverySettingsV1 {
                daily_proposal_limit: 10,
                ..defaults.clone()
            },
            defaults.revision,
        )
        .unwrap();
    let runs = SelfImprovementStore::new(&store);

    let (root_decision, root_candidate) = pursue(&opportunities, opportunity());
    let root = runs
        .create_from_pursued(&opportunities, &root_decision, root_candidate)
        .unwrap();
    assert_eq!(root.parent_run_uid, None);
    assert_eq!(root.chain_depth, 0);

    let (child_decision, child_candidate) =
        pursue(&opportunities, linked_proposal(&root.feature_uid));
    assert!(
        runs.run_for_feature(&child_candidate.feature_uid)
            .unwrap()
            .is_none()
    );
    let child = runs
        .create_from_pursued(&opportunities, &child_decision, child_candidate)
        .unwrap();
    assert_eq!(child.parent_run_uid.as_deref(), Some(root.run_uid.as_str()));
    assert_eq!(child.chain_depth, 1);

    runs.apply_event(
        &child.run_uid,
        child.revision,
        &uuid::Uuid::new_v4().to_string(),
        SelfImprovementEventV1::Cancel,
    )
    .unwrap();
    let (grandchild_decision, grandchild_candidate) =
        pursue(&opportunities, linked_proposal(&child.feature_uid));
    let grandchild = runs
        .create_from_pursued(&opportunities, &grandchild_decision, grandchild_candidate)
        .unwrap();
    assert_eq!(
        grandchild.parent_run_uid.as_deref(),
        Some(child.run_uid.as_str())
    );
    assert_eq!(
        grandchild.chain_depth,
        crate::core::self_improvement::MAX_SELF_IMPROVEMENT_CHAIN_DEPTH
    );

    runs.apply_event(
        &grandchild.run_uid,
        grandchild.revision,
        &uuid::Uuid::new_v4().to_string(),
        SelfImprovementEventV1::Cancel,
    )
    .unwrap();
    let (blocked_decision, blocked_candidate) =
        pursue(&opportunities, linked_proposal(&grandchild.feature_uid));
    let error = runs
        .create_from_pursued(&opportunities, &blocked_decision, blocked_candidate.clone())
        .unwrap_err();
    assert!(error.to_string().contains("chain depth limit"));
    assert!(
        runs.run_for_feature(&blocked_candidate.feature_uid)
            .unwrap()
            .is_none()
    );

    let (sibling_decision, sibling_candidate) =
        pursue(&opportunities, linked_proposal(&root.feature_uid));
    let sibling = runs
        .create_from_pursued(&opportunities, &sibling_decision, sibling_candidate)
        .unwrap();
    assert_eq!(sibling.chain_depth, 1);

    let grandchild_path = format!("state/self_improvements/{}.json", grandchild.run_uid);
    let (mut persisted, revision) = store
        .read_record::<serde_json::Value>(&grandchild_path)
        .unwrap();
    let tampered_revision = revision + 1;
    persisted["revision"] = serde_json::json!(tampered_revision);
    persisted["data"]["revision"] = serde_json::json!(tampered_revision);
    persisted["data"]["parent_run_uid"] = serde_json::json!(sibling.run_uid);
    store
        .transaction_with_record_revisions(
            &[(
                grandchild_path.clone(),
                serde_json::to_vec(&persisted).unwrap(),
            )],
            &[crate::artifacts::planning_store::RecordRevisionCheck {
                path: grandchild_path.clone(),
                expected_revision: revision,
            }],
        )
        .unwrap();
    assert!(runs.load(&grandchild.run_uid).is_err());

    let (mut erased, revision) = store
        .read_record::<serde_json::Value>(&grandchild_path)
        .unwrap();
    let erased_revision = revision + 1;
    erased["revision"] = serde_json::json!(erased_revision);
    erased["data"]["revision"] = serde_json::json!(erased_revision);
    erased["data"]["parent_run_uid"] = serde_json::Value::Null;
    erased["data"]["chain_depth"] = serde_json::json!(0);
    store
        .transaction_with_record_revisions(
            &[(
                grandchild_path.clone(),
                serde_json::to_vec(&erased).unwrap(),
            )],
            &[crate::artifacts::planning_store::RecordRevisionCheck {
                path: grandchild_path,
                expected_revision: revision,
            }],
        )
        .unwrap();
    assert!(runs.load(&grandchild.run_uid).is_err());

    let (recovery_decision, recovery_candidate) =
        pursue(&opportunities, linked_proposal(&grandchild.feature_uid));
    let recovery_run_path = format!(
        "state/self_improvements/{}.json",
        SelfImprovementStore::run_uid_for_decision(&recovery_decision).unwrap()
    );
    assert!(
        runs.create_from_pursued(
            &opportunities,
            &recovery_decision,
            recovery_candidate.clone()
        )
        .is_err()
    );
    assert!(
        store
            .read_record::<serde_json::Value>(&recovery_run_path)
            .is_err()
    );
    assert_eq!(
        opportunities
            .load(&recovery_candidate.opportunity_uid)
            .unwrap()
            .0
            .status,
        OpportunityStatusV1::ApprovedToPlan
    );
}
