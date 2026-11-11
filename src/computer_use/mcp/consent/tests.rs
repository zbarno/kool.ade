use super::*;
use std::sync::atomic::AtomicBool;

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
        target: crate::computer_use::ComputerTargetV1::LocalKvm {
            machine_id: "00000000-0000-4000-8000-000000000011".into(),
        },
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
fn explicit_consent_carries_the_exact_app_selected_remote_worker_target() {
    let broker = ComputerUseConsentBrokerV1::default();
    let mut selected_scope = scope();
    let worker_key = ed25519_dalek::SigningKey::from_bytes(&[17; 32]);
    let machine_id = "00000000-0000-4000-8000-000000000098";
    selected_scope.machine_id = machine_id.into();
    selected_scope.target = crate::computer_use::ComputerTargetV1::RemoteVmWorker {
        worker_id: "worker-east".into(),
        machine_id: machine_id.into(),
        trust: crate::computer_use::ComputerRemoteWorkerTrustV1 {
            ssh_host_key_fingerprint: format!("SHA256:{}", "A".repeat(43)),
            worker_attestation_key_id: "00000000-0000-4000-8000-000000000099".into(),
            worker_attestation_verifying_key_hex: worker_key
                .verifying_key()
                .to_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            transport_protocol_version:
                crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
            guest_protocol_version: crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
        },
    };
    let cancelled = AtomicBool::new(false);
    let requester_broker = broker.clone();
    let requester = std::thread::spawn(move || {
        requester_broker.request(
            &caller(),
            "inspect the approved remote fixture",
            selected_scope,
            &cancelled,
        )
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let pending = loop {
        if let Some(request) = broker.pending().into_iter().next() {
            break request;
        }
        assert!(Instant::now() < deadline, "remote consent did not arrive");
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(pending.target.worker_id(), "worker-east");
    assert_eq!(pending.target.target_machine_id(), machine_id);
    broker.decide(&pending.request_id, true).unwrap();
    let ComputerUseConsentDecisionV1::Approved(grant) = requester.join().unwrap() else {
        panic!("explicit remote consent should produce a grant");
    };
    assert_eq!(grant.request.target, pending.target);
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

#[test]
fn runtime_qa_role_is_limited_to_one_task_app_and_gui_operations() {
    let mut caller = caller();
    caller.role_id = super::super::COMPUTER_USE_RUNTIME_QA_ROLE_V1.into();
    let mut scope = scope();
    scope.allowed_apps = vec![super::super::COMPUTER_USE_RUNTIME_QA_APP_ID_V1.into()];
    scope.allowed_operations = super::super::RUNTIME_QA_COMPUTER_USE_OPERATIONS_V1.to_vec();
    assert!(scope.validate_for_role(&caller.role_id).is_ok());

    let mut widened = scope.clone();
    widened.allowed_apps.push("chromium".into());
    assert!(widened.validate_for_role(&caller.role_id).is_err());

    let mut catalog_launch = scope.clone();
    catalog_launch
        .allowed_operations
        .push(crate::computer_use::ComputerOperationV1::LaunchAllowedApp);
    assert!(catalog_launch.validate_for_role(&caller.role_id).is_err());

    let mut app_management = scope;
    app_management
        .allowed_operations
        .push(crate::computer_use::ComputerOperationV1::ManageApp);
    assert!(app_management.validate_for_role(&caller.role_id).is_err());
}
