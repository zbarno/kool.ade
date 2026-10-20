use super::*;

#[test]
fn pursued_product_decision_creates_one_durable_run_and_replay_is_idempotent() {
    let temp = TempDir::new();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.0.join("planning"),
        StoreMode::ManagedLocal,
    );
    let opportunities = OpportunityStore::new(&store);
    let proposal = opportunity();
    let saved = opportunities.save_advisory(proposal).unwrap();
    let OpportunitySaveResult::Created {
        proposal,
        record_revision,
    } = saved
    else {
        panic!("first proposal should be created")
    };
    let decision = decision_for(&proposal);
    let decision_uid = decision.uid.clone();
    opportunities
        .record_user_decision(record_revision, &decision, "local-controller")
        .unwrap();
    let initial = run(&decision_uid, &proposal);
    let runs = SelfImprovementStore::new(&store);
    let created = runs
        .create_from_pursued(&opportunities, &decision_uid, initial.clone())
        .unwrap();
    assert_eq!(created.revision, 2);
    assert_eq!(created.phase, SelfImprovementPhaseV1::Planning);
    assert_eq!(runs.load(&created.run_uid).unwrap(), created);
    let (linked, _) = opportunities.load(&proposal.opportunity_uid).unwrap();
    assert_eq!(linked.status, OpportunityStatusV1::Planning);
    assert!(linked.related_feature_uids.contains(&created.feature_uid));
    assert_eq!(
        runs.create_from_pursued(&opportunities, &decision_uid, initial)
            .unwrap(),
        created
    );
    assert_eq!(runs.transitions(&created.run_uid).unwrap().len(), 2);
}

#[test]
fn self_improvement_runs_keep_each_users_persisted_pursue_provenance() {
    let temp = TempDir::new();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.0.join("planning"),
        StoreMode::ManagedLocal,
    );
    let opportunities = OpportunityStore::new(&store);
    let runs = SelfImprovementStore::new(&store);
    let users = ["synthetic-user-a", "synthetic-user-b"];
    let mut receipts = Vec::new();

    for user in users {
        let OpportunitySaveResult::Created {
            proposal,
            record_revision,
        } = opportunities.save_advisory(opportunity()).unwrap()
        else {
            panic!("each synthetic opportunity should be created")
        };
        let mut decision = decision_for(&proposal);
        decision.user_id = user.into();
        opportunities
            .record_user_decision(record_revision, &decision, user)
            .unwrap();
        let created = runs
            .create_from_pursued(&opportunities, &decision.uid, run(&decision.uid, &proposal))
            .unwrap();
        receipts.push((user, decision.uid, proposal.opportunity_uid, created));
    }

    for (user, decision_uid, opportunity_uid, run) in &receipts {
        let decision = opportunities.decision(decision_uid).unwrap();
        assert_eq!(decision.user_id, *user);
        assert_eq!(decision.opportunity_uid, *opportunity_uid);
        assert_eq!(run.approved_decision_uid, *decision_uid);
        assert_eq!(run.opportunity_uid, *opportunity_uid);
        assert_eq!(runs.load(&run.run_uid).unwrap(), *run);
    }

    let (_, _, _, first_run) = &receipts[0];
    let (_, second_decision, _, _) = &receipts[1];
    let forged_cross_user_run = SelfImprovementRunV1 {
        approved_decision_uid: second_decision.clone(),
        ..first_run.clone()
    };
    assert!(
        runs.create_from_pursued(&opportunities, second_decision, forged_cross_user_run,)
            .is_err(),
        "a user's Pursue receipt cannot authorize another user's opportunity/run"
    );
}

#[test]
fn active_run_limit_is_durable_blocks_new_pursuit_and_releases_after_cancel() {
    let temp = TempDir::new();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.0.join("planning"),
        StoreMode::ManagedLocal,
    );
    let opportunities = OpportunityStore::new(&store);
    let defaults = opportunities.discovery_settings().unwrap();
    let settings = crate::core::opportunity::OpportunityDiscoverySettingsV1 {
        active_self_improvement_limit: 1,
        ..defaults.clone()
    };
    opportunities
        .save_discovery_settings(&settings, defaults.revision)
        .unwrap();

    let (first_decision, first_run) = pursue(&opportunities, opportunity());
    let (second_decision, second_run) = pursue(&opportunities, opportunity());
    let runs = SelfImprovementStore::new(&store);
    let first = runs
        .create_from_pursued(&opportunities, &first_decision, first_run)
        .unwrap();
    let error = runs
        .create_from_pursued(&opportunities, &second_decision, second_run.clone())
        .unwrap_err();
    assert!(error.to_string().contains("active self-improvement limit"));

    let cancelled = runs
        .apply_event(
            &first.run_uid,
            first.revision,
            &uuid::Uuid::new_v4().to_string(),
            SelfImprovementEventV1::Cancel,
        )
        .unwrap();
    assert_eq!(cancelled.phase, SelfImprovementPhaseV1::Cancelled);
    assert!(
        runs.apply_event(
            &cancelled.run_uid,
            cancelled.revision,
            &uuid::Uuid::new_v4().to_string(),
            SelfImprovementEventV1::NeedsAttention,
        )
        .is_err(),
        "a cancelled run cannot be reopened through NeedsAttention"
    );
    let created = SelfImprovementStore::new(&store)
        .create_from_pursued(&opportunities, &second_decision, second_run)
        .unwrap();
    assert_eq!(created.phase, SelfImprovementPhaseV1::Planning);
}

#[test]
fn concurrent_pursuits_cannot_reserve_more_active_runs_than_the_limit() {
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
                active_self_improvement_limit: 1,
                ..defaults.clone()
            },
            defaults.revision,
        )
        .unwrap();
    let first = pursue(&opportunities, opportunity());
    let second = pursue(&opportunities, opportunity());
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let first_store = &store;
    let (first_decision, first_run) = first;
    let second_store = &store;
    let (second_decision, second_run) = second;

    let results = std::thread::scope(|scope| {
        let first_barrier = barrier.clone();
        let first_handle = scope.spawn(move || {
            first_barrier.wait();
            SelfImprovementStore::new(first_store).create_from_pursued(
                &OpportunityStore::new(first_store),
                &first_decision,
                first_run,
            )
        });
        let second_barrier = barrier.clone();
        let second_handle = scope.spawn(move || {
            second_barrier.wait();
            SelfImprovementStore::new(second_store).create_from_pursued(
                &OpportunityStore::new(second_store),
                &second_decision,
                second_run,
            )
        });
        [first_handle.join().unwrap(), second_handle.join().unwrap()]
    });

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .any(|error| error.to_string().contains("active self-improvement limit"))
    );
}

#[test]
fn missing_active_index_is_rebuilt_from_durable_runs_before_reserving() {
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
                active_self_improvement_limit: 1,
                ..defaults.clone()
            },
            defaults.revision,
        )
        .unwrap();
    let (decision_uid, initial_run) = pursue(&opportunities, opportunity());
    let runs = SelfImprovementStore::new(&store);
    runs.create_from_pursued(&opportunities, &decision_uid, initial_run)
        .unwrap();
    std::fs::remove_file(
        temp.0
            .join("planning/state/self_improvement_active_runs.json"),
    )
    .unwrap();

    let (next_decision_uid, next_run) = pursue(&opportunities, opportunity());
    let error = SelfImprovementStore::new(&store)
        .create_from_pursued(&opportunities, &next_decision_uid, next_run)
        .unwrap_err();
    assert!(error.to_string().contains("active self-improvement limit"));
}

#[path = "creation_tests/lineage.rs"]
mod lineage;
