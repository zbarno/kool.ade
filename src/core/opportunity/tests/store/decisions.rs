use super::*;

#[test]
fn authenticated_decision_is_durable_and_idempotent_but_rejects_forgery_and_stale_revision() {
    let (_temp, store) = setup();
    let opportunities = OpportunityStore::new(&store);
    let OpportunitySaveResult::Created {
        proposal,
        record_revision,
    } = opportunities.save_advisory(proposal()).unwrap()
    else {
        panic!("proposal should be created")
    };
    let decision = prompt_decision(&proposal, "synthetic-user", OpportunityChoice::Pursue);
    let forged = UiDecision {
        user_id: "different-user".into(),
        ..decision.clone()
    };
    assert!(
        opportunities
            .record_user_decision(record_revision, &forged, "synthetic-user")
            .is_err()
    );
    assert!(matches!(
        opportunities.record_user_decision(record_revision + 1, &decision, "synthetic-user"),
        Err(crate::artifacts::planning_store::StoreError::StaleRecordRevision { .. })
    ));

    let recorded = opportunities
        .record_user_decision(record_revision, &decision, "synthetic-user")
        .unwrap();
    let OpportunityDecisionResult::Recorded {
        proposal: updated, ..
    } = recorded
    else {
        panic!("first explicit user choice should be recorded")
    };
    assert_eq!(updated.status, OpportunityStatusV1::ApprovedToPlan);
    let reopened = OpportunityStore::new(&store);
    assert_eq!(reopened.load(&proposal.opportunity_uid).unwrap().0, updated);
    assert_eq!(
        reopened.decision(&decision.uid).unwrap().user_id,
        "synthetic-user"
    );
    assert!(matches!(
        reopened.record_user_decision(record_revision, &decision, "synthetic-user"),
        Ok(OpportunityDecisionResult::AlreadyRecorded { .. })
    ));

    let forged_reuse = UiDecision {
        explanation: Some("altered receipt".into()),
        ..decision
    };
    assert!(
        reopened
            .record_user_decision(record_revision, &forged_reuse, "synthetic-user")
            .is_err()
    );
}

#[test]
fn all_explicit_choices_persist_their_distinct_lifecycle_state() {
    let cases = [
        (
            OpportunityChoice::Pursue,
            OpportunityStatusV1::ApprovedToPlan,
        ),
        (OpportunityChoice::NotNow, OpportunityStatusV1::Deferred),
        (OpportunityChoice::Decline, OpportunityStatusV1::Declined),
        (
            OpportunityChoice::AskForMoreInformation,
            OpportunityStatusV1::InformationRequested,
        ),
    ];
    for (choice, expected_status) in cases {
        let (_temp, store) = setup();
        let opportunities = OpportunityStore::new(&store);
        let OpportunitySaveResult::Created {
            proposal,
            record_revision,
        } = opportunities.save_advisory(proposal()).unwrap()
        else {
            panic!("proposal should be created")
        };
        let decision = prompt_decision(&proposal, "synthetic-user", choice);
        let recorded = opportunities
            .record_user_decision(record_revision, &decision, "synthetic-user")
            .unwrap();
        let OpportunityDecisionResult::Recorded { proposal, .. } = recorded else {
            panic!("explicit choice should create a receipt")
        };
        assert_eq!(proposal.status, expected_status);
    }
}

#[test]
fn not_now_suppresses_choices_until_expiry_then_allows_an_explicit_follow_up() {
    let (_temp, store) = setup();
    let opportunities = OpportunityStore::new(&store);
    let OpportunitySaveResult::Created {
        proposal,
        record_revision,
    } = opportunities.save_advisory(proposal()).unwrap()
    else {
        panic!("proposal should be created")
    };
    let mut snooze = prompt_decision(&proposal, "synthetic-user", OpportunityChoice::NotNow);
    snooze.snooze_until = Some("2026-10-20T12:00:00Z".into());
    let before_expiry = chrono::DateTime::parse_from_rfc3339("2026-10-19T12:00:00Z")
        .unwrap()
        .timestamp_millis() as u64;
    let OpportunityDecisionResult::Recorded { proposal, .. } = opportunities
        .record_user_decision_at(record_revision, &snooze, "synthetic-user", before_expiry)
        .unwrap()
    else {
        panic!("Not now should be recorded")
    };
    assert_eq!(proposal.status, OpportunityStatusV1::Deferred);
    let (proposal, current_revision) = opportunities.load(&proposal.opportunity_uid).unwrap();
    let early = prompt_decision(&proposal, "synthetic-user", OpportunityChoice::Pursue);
    assert!(
        opportunities
            .record_user_decision_at(current_revision, &early, "synthetic-user", before_expiry)
            .is_err()
    );

    let mut after_expiry = prompt_decision(&proposal, "synthetic-user", OpportunityChoice::Pursue);
    after_expiry.occurred_at = "2026-10-20T12:00:00Z".into();
    let after_expiry_ms = chrono::DateTime::parse_from_rfc3339("2026-10-20T12:00:00Z")
        .unwrap()
        .timestamp_millis() as u64;
    let OpportunityDecisionResult::Recorded { proposal, .. } = opportunities
        .record_user_decision_at(
            current_revision,
            &after_expiry,
            "synthetic-user",
            after_expiry_ms,
        )
        .unwrap()
    else {
        panic!("an expired snooze should allow a fresh explicit choice")
    };
    assert_eq!(proposal.status, OpportunityStatusV1::ApprovedToPlan);
}

#[test]
fn an_information_request_can_be_followed_by_a_new_user_choice() {
    let (_temp, store) = setup();
    let opportunities = OpportunityStore::new(&store);
    let OpportunitySaveResult::Created {
        proposal,
        record_revision,
    } = opportunities.save_advisory(proposal()).unwrap()
    else {
        panic!("proposal should be created")
    };

    let ask = prompt_decision(
        &proposal,
        "synthetic-user",
        OpportunityChoice::AskForMoreInformation,
    );
    let OpportunityDecisionResult::Recorded {
        proposal: information_requested,
        record_revision,
        ..
    } = opportunities
        .record_user_decision(record_revision, &ask, "synthetic-user")
        .unwrap()
    else {
        panic!("Ask must be recorded as a distinct receipt")
    };
    assert_eq!(
        information_requested.status,
        OpportunityStatusV1::InformationRequested
    );

    let mut pursue = prompt_decision(
        &information_requested,
        "synthetic-user",
        OpportunityChoice::Pursue,
    );
    pursue.occurred_at = "2026-10-13T12:00:01Z".into();
    let OpportunityDecisionResult::Recorded {
        proposal: approved,
        decision,
        ..
    } = opportunities
        .record_user_decision(record_revision, &pursue, "synthetic-user")
        .unwrap()
    else {
        panic!("a later explicit user choice must be recorded")
    };
    assert_eq!(approved.status, OpportunityStatusV1::ApprovedToPlan);
    assert_eq!(opportunities.list_decisions().unwrap().len(), 2);
    assert_eq!(
        opportunities
            .decision_for_opportunity(&proposal.opportunity_uid)
            .unwrap(),
        Some(decision)
    );
}

#[test]
fn pursued_opportunity_advances_to_planning_with_feature_identity() {
    let (_temp, store) = setup();
    let opportunities = OpportunityStore::new(&store);
    let OpportunitySaveResult::Created {
        proposal,
        record_revision,
    } = opportunities.save_advisory(proposal()).unwrap()
    else {
        panic!("proposal should be created")
    };
    let decision = prompt_decision(&proposal, "local-controller", OpportunityChoice::Pursue);
    let OpportunityDecisionResult::Recorded {
        record_revision, ..
    } = opportunities
        .record_user_decision(record_revision, &decision, "local-controller")
        .unwrap()
    else {
        panic!("Pursue must record a receipt")
    };
    let feature_uid = uuid::Uuid::new_v4().to_string();
    let (planned, next_revision) = opportunities
        .mark_planning(&proposal.opportunity_uid, &feature_uid, record_revision)
        .unwrap();
    assert_eq!(planned.status, OpportunityStatusV1::Planning);
    assert_eq!(planned.related_feature_uids, [feature_uid]);
    let reopened = OpportunityStore::new(&store);
    let (persisted, persisted_revision) = reopened.load(&proposal.opportunity_uid).unwrap();
    assert_eq!(persisted, planned);
    assert_eq!(persisted_revision, next_revision);
}
