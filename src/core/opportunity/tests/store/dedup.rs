use super::*;

#[test]
fn a_declined_proposal_reopens_only_for_new_evidence() {
    let (_temp, store) = setup();
    let opportunities = OpportunityStore::new(&store);
    let first = proposal();
    let OpportunitySaveResult::Created {
        proposal: saved,
        record_revision,
    } = opportunities.save_advisory(first.clone()).unwrap()
    else {
        panic!("first proposal should be created")
    };
    let decline = prompt_decision(&saved, "synthetic-user", OpportunityChoice::Decline);
    opportunities
        .record_user_decision(record_revision, &decline, "synthetic-user")
        .unwrap();

    let mut repeated = first.clone();
    repeated.opportunity_uid = uuid::Uuid::new_v4().to_string();
    repeated.origin = crate::core::opportunity::OpportunityOriginV1::UserGoal;
    assert!(matches!(
        opportunities.save_advisory(repeated).unwrap(),
        OpportunitySaveResult::Duplicate { .. }
    ));

    let mut new_evidence = first;
    new_evidence.opportunity_uid = uuid::Uuid::new_v4().to_string();
    new_evidence
        .evidence_refs
        .push(crate::core::opportunity::OpportunityEvidenceRefV1 {
            evidence_uid: uuid::Uuid::new_v4().to_string(),
            sha256: "d".repeat(64),
            confidentiality: crate::core::opportunity::EvidenceConfidentialityV1::UserLocal,
        });
    let reopened = opportunities.save_advisory(new_evidence).unwrap();
    let OpportunitySaveResult::Created {
        proposal: reopened, ..
    } = reopened
    else {
        panic!("new evidence should create a fresh proposal")
    };
    assert_eq!(
        opportunities.load(&saved.opportunity_uid).unwrap().0.status,
        OpportunityStatusV1::Declined
    );
    assert_eq!(
        opportunities
            .load(&reopened.opportunity_uid)
            .unwrap()
            .0
            .status,
        OpportunityStatusV1::AwaitingDecision
    );
    let mut repeated_new_evidence = reopened.clone();
    repeated_new_evidence.opportunity_uid = uuid::Uuid::new_v4().to_string();
    assert!(matches!(
        opportunities
            .save_advisory(repeated_new_evidence)
            .unwrap(),
        OpportunitySaveResult::Duplicate { proposal, .. }
            if proposal.opportunity_uid == reopened.opportunity_uid
    ));
}

#[test]
fn regenerated_legacy_evidence_id_with_same_hash_does_not_reopen_declined_proposal() {
    let (_temp, store) = setup();
    let opportunities = OpportunityStore::new(&store);
    let first = proposal();
    let OpportunitySaveResult::Created {
        proposal: saved,
        record_revision,
    } = opportunities.save_advisory(first.clone()).unwrap()
    else {
        panic!("first proposal should be created")
    };
    let decline = prompt_decision(&saved, "synthetic-user", OpportunityChoice::Decline);
    opportunities
        .record_user_decision(record_revision, &decline, "synthetic-user")
        .unwrap();

    let mut regenerated = first;
    regenerated.opportunity_uid = uuid::Uuid::new_v4().to_string();
    regenerated.evidence_refs[0].evidence_uid = uuid::Uuid::new_v4().to_string();
    assert!(matches!(
        opportunities.save_advisory(regenerated).unwrap(),
        OpportunitySaveResult::Duplicate { proposal, .. }
            if proposal.opportunity_uid == saved.opportunity_uid
    ));
}

#[test]
fn new_evidence_does_not_bypass_an_active_not_now_snooze() {
    let (_temp, store) = setup();
    let opportunities = OpportunityStore::new(&store);
    let first = proposal();
    let OpportunitySaveResult::Created {
        proposal: saved,
        record_revision,
    } = opportunities.save_advisory(first.clone()).unwrap()
    else {
        panic!("first proposal should be created")
    };
    let mut defer = prompt_decision(&saved, "synthetic-user", OpportunityChoice::NotNow);
    defer.occurred_at = "2099-01-01T00:00:00Z".into();
    defer.snooze_until = Some("2099-01-08T00:00:00Z".into());
    opportunities
        .record_user_decision(record_revision, &defer, "synthetic-user")
        .unwrap();

    let mut new_evidence = first;
    new_evidence.opportunity_uid = uuid::Uuid::new_v4().to_string();
    new_evidence
        .evidence_refs
        .push(crate::core::opportunity::OpportunityEvidenceRefV1 {
            evidence_uid: uuid::Uuid::new_v4().to_string(),
            sha256: "e".repeat(64),
            confidentiality: crate::core::opportunity::EvidenceConfidentialityV1::UserLocal,
        });
    assert!(matches!(
        opportunities.save_advisory(new_evidence).unwrap(),
        OpportunitySaveResult::Duplicate { proposal, .. }
            if proposal.opportunity_uid == saved.opportunity_uid
    ));
}

#[test]
fn fresh_evidence_reopens_a_deferred_proposal_after_its_snooze_expires() {
    let (_temp, store) = setup();
    let opportunities = OpportunityStore::new(&store);
    let first = proposal();
    let OpportunitySaveResult::Created {
        proposal: saved,
        record_revision,
    } = opportunities.save_advisory(first.clone()).unwrap()
    else {
        panic!("first proposal should be created")
    };
    let mut defer = prompt_decision(&saved, "synthetic-user", OpportunityChoice::NotNow);
    defer.occurred_at = "2000-01-01T00:00:00Z".into();
    defer.snooze_until = Some("2000-01-08T00:00:00Z".into());
    opportunities
        .record_user_decision(record_revision, &defer, "synthetic-user")
        .unwrap();

    let mut new_evidence = first;
    new_evidence.opportunity_uid = uuid::Uuid::new_v4().to_string();
    new_evidence
        .evidence_refs
        .push(crate::core::opportunity::OpportunityEvidenceRefV1 {
            evidence_uid: uuid::Uuid::new_v4().to_string(),
            sha256: "f".repeat(64),
            confidentiality: crate::core::opportunity::EvidenceConfidentialityV1::UserLocal,
        });
    assert!(matches!(
        opportunities.save_advisory(new_evidence).unwrap(),
        OpportunitySaveResult::Created { .. }
    ));
    assert_eq!(
        opportunities.load(&saved.opportunity_uid).unwrap().0.status,
        OpportunityStatusV1::Deferred
    );
}

#[test]
fn concurrent_new_snooze_invalidates_a_previously_eligible_reopen() {
    let (_temp, store) = setup();
    let opportunities = OpportunityStore::new(&store);
    let OpportunitySaveResult::Created {
        proposal: saved,
        record_revision,
    } = opportunities.save_advisory(proposal()).unwrap()
    else {
        panic!("first proposal should be created")
    };
    let mut expired_snooze = prompt_decision(&saved, "synthetic-user", OpportunityChoice::NotNow);
    expired_snooze.occurred_at = "2000-01-01T00:00:00Z".into();
    expired_snooze.snooze_until = Some("2000-01-08T00:00:00Z".into());
    let OpportunityDecisionResult::Recorded {
        proposal: deferred,
        record_revision: eligible_revision,
        ..
    } = opportunities
        .record_user_decision(record_revision, &expired_snooze, "synthetic-user")
        .unwrap()
    else {
        panic!("expired snooze should be recorded")
    };

    // This stands in for a new NotNow choice landing after the reopen read,
    // but before its transaction commits.
    let mut fresh_evidence = proposal();
    fresh_evidence.opportunity_uid = uuid::Uuid::new_v4().to_string();
    fresh_evidence.workspace_uid = deferred.workspace_uid.clone();
    fresh_evidence.target = deferred.target.clone();
    fresh_evidence.problem_or_opportunity = deferred.problem_or_opportunity.clone();
    fresh_evidence
        .evidence_refs
        .push(crate::core::opportunity::OpportunityEvidenceRefV1 {
            evidence_uid: uuid::Uuid::new_v4().to_string(),
            sha256: "f".repeat(64),
            confidentiality: crate::core::opportunity::EvidenceConfidentialityV1::UserLocal,
        });
    assert_eq!(
        crate::core::opportunity::dedup_fingerprint(
            &fresh_evidence.workspace_uid,
            &fresh_evidence.target.repository_uid,
            &fresh_evidence.problem_or_opportunity,
        )
        .unwrap(),
        deferred.dedup_fingerprint
    );
    let mut renewed = prompt_decision(&deferred, "synthetic-user", OpportunityChoice::NotNow);
    renewed.occurred_at = "2026-10-17T12:00:00Z".into();
    renewed.snooze_until = Some("2026-11-16T12:00:00Z".into());
    let result = opportunities
        .save_advisory_record_with_before_commit(fresh_evidence, None, || {
            opportunities
                .record_user_decision(eligible_revision, &renewed, "synthetic-user")
                .map(|_| ())
        })
        .unwrap();
    assert!(
        matches!(
            &result,
            OpportunitySaveResult::Duplicate { proposal, .. }
                if proposal.opportunity_uid == deferred.opportunity_uid
        ),
        "reopen should observe the concurrent snooze; got {result:?}"
    );
}
