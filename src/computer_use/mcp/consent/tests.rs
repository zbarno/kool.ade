use super::*;

fn caller() -> ComputerUseCallerV1 {
    ComputerUseCallerV1 {
        identity: crate::computer_use::ComputerIdentityV1 {
            project_id: "00000000-0000-4000-8000-000000000001".into(),
            task_id: "00000000-0000-4000-8000-000000000002".into(),
            user_id: "00000000-0000-4000-8000-000000000003".into(),
            session_id: "00000000-0000-4000-8000-000000000004".into(),
        },
        actor: crate::computer_use::ComputerActionActorV1::Agent,
        role_id: "implementer/v1".into(),
    }
}

fn scope() -> ComputerUseConsentScopeV1 {
    ComputerUseConsentScopeV1 {
        machine_id: "00000000-0000-4000-8000-000000000011".into(),
        approved_destination: None,
        allowed_operations: vec![crate::computer_use::ComputerOperationV1::ObserveScreen],
        allowed_apps: vec!["chromium".into(), "test-gui-fixture".into()],
        network_profile: crate::computer_use::ComputerNetworkProfileV1::Disabled,
        internet_research_available: true,
        resource_budget: crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1,
    }
}

#[test]
fn only_an_explicit_ui_decision_approves_a_pending_session_request() {
    let broker = ComputerUseConsentBrokerV1::default();
    let thread_broker = broker.clone();
    let cancellation = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thread_cancellation = cancellation.clone();
    let requester = std::thread::spawn(move || {
        thread_broker.request(
            &caller(),
            "inspect the synthetic settings window",
            scope(),
            &thread_cancellation,
        )
    });

    let deadline = Instant::now() + Duration::from_secs(2);
    let pending = loop {
        if let Some(request) = broker.pending().into_iter().next() {
            break request;
        }
        assert!(
            Instant::now() < deadline,
            "consent request was not enqueued"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(pending.role_id, "implementer/v1");
    assert_eq!(pending.purpose, "inspect the synthetic settings window");
    assert_eq!(pending.machine_id, "00000000-0000-4000-8000-000000000011");
    assert_eq!(pending.approved_destination, None);
    broker.decide(&pending.request_id, true).unwrap();
    let ComputerUseConsentDecisionV1::Approved(grant) = requester.join().unwrap() else {
        panic!("explicit consent should return a scoped grant");
    };
    assert_eq!(grant.request.identity, caller().identity);
    assert_eq!(grant.request.role_id, "implementer/v1");
    assert_eq!(
        grant.request.network_profile,
        crate::computer_use::ComputerNetworkProfileV1::Disabled
    );
    assert_eq!(
        grant.request.resource_budget,
        crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1
    );
    assert!(
        grant
            .request
            .allowed_apps
            .contains(&"test-gui-fixture".into())
    );
    assert!(grant.expires_at_unix_ms > grant.approved_at_unix_ms);
    assert!(broker.pending().is_empty());
}

#[test]
fn internet_research_requires_a_separate_offered_human_choice() {
    let broker = ComputerUseConsentBrokerV1::default();
    let thread_broker = broker.clone();
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thread_cancelled = cancelled.clone();
    let requester = std::thread::spawn(move || {
        thread_broker.request(
            &caller(),
            "research a public product page",
            scope(),
            &thread_cancelled,
        )
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let pending = loop {
        if let Some(request) = broker.pending().into_iter().next() {
            break request;
        }
        assert!(
            Instant::now() < deadline,
            "consent request was not enqueued"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(
        pending.available_network_profiles,
        [
            crate::computer_use::ComputerNetworkProfileV1::Disabled,
            crate::computer_use::ComputerNetworkProfileV1::InternetResearch,
        ]
    );
    broker
        .decide_with_network_profile(
            &pending.request_id,
            crate::computer_use::ComputerNetworkProfileV1::InternetResearch,
        )
        .unwrap();
    let ComputerUseConsentDecisionV1::Approved(grant) = requester.join().unwrap() else {
        panic!("explicit research choice should return a scoped grant");
    };
    assert_eq!(
        grant.request.network_profile,
        crate::computer_use::ComputerNetworkProfileV1::InternetResearch
    );
    assert!(
        grant
            .request
            .allowed_apps
            .contains(&"chromium-research".into())
    );
    assert!(!grant.request.allowed_apps.contains(&"chromium".into()));
}

#[test]
fn unoffered_internet_research_cannot_be_granted_or_consume_the_pending_request() {
    let broker = ComputerUseConsentBrokerV1::default();
    let thread_broker = broker.clone();
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thread_cancelled = cancelled.clone();
    let requester = std::thread::spawn(move || {
        let mut scope = scope();
        scope.internet_research_available = false;
        thread_broker.request(
            &caller(),
            "synthetic offline request",
            scope,
            &thread_cancelled,
        )
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let pending = loop {
        if let Some(request) = broker.pending().into_iter().next() {
            break request;
        }
        assert!(
            Instant::now() < deadline,
            "consent request was not enqueued"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(
        broker
            .decide_with_network_profile(
                &pending.request_id,
                crate::computer_use::ComputerNetworkProfileV1::InternetResearch,
            )
            .is_err()
    );
    assert_eq!(broker.pending().len(), 1);
    broker.decide(&pending.request_id, true).unwrap();
    let ComputerUseConsentDecisionV1::Approved(grant) = requester.join().unwrap() else {
        panic!("offline consent should remain available");
    };
    assert_eq!(
        grant.request.network_profile,
        crate::computer_use::ComputerNetworkProfileV1::Disabled
    );
}

#[test]
fn cancellation_cannot_leave_a_stale_consent_request() {
    let broker = ComputerUseConsentBrokerV1::default();
    let cancelled = std::sync::atomic::AtomicBool::new(true);

    assert_eq!(
        broker.request(&caller(), "synthetic request", scope(), &cancelled),
        ComputerUseConsentDecisionV1::Cancelled
    );
    assert!(broker.pending().is_empty());
}

#[test]
fn scope_proposal_rejects_unapproved_destinations_network_and_excess_budget() {
    let mut invalid_destination = scope();
    invalid_destination.approved_destination = Some("https://example.invalid".into());
    assert!(invalid_destination.validate().is_err());

    let mut invalid_network = scope();
    invalid_network.network_profile =
        crate::computer_use::ComputerNetworkProfileV1::InternetResearch;
    assert!(invalid_network.validate().is_err());

    let mut excessive_budget = scope();
    excessive_budget.resource_budget.cpu_millis += 1;
    assert!(excessive_budget.validate().is_err());

    let mut invalid_target = scope();
    invalid_target.machine_id = "machine-from-model".into();
    assert!(invalid_target.validate().is_err());
}
