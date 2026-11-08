use super::*;
use crate::computer_use::mcp::ComputerUseConsentBrokerV1;

#[test]
fn issued_session_is_accepted_only_when_its_scope_fits_explicit_consent() {
    let caller = caller();
    let now = unix_time_ms();
    let grant = approved_grant(&caller, now);
    let registry = Arc::new(ComputerUseAuthorityRegistryV1::default());
    register_session(&registry, &caller, &grant, false);
    let host = RegistryBoundComputerUseMcpHostV1::new(
        registry.clone(),
        ComputerUseConsentBrokerV1::default(),
        Arc::new(Operations(AtomicUsize::new(0))),
    );
    let ready = Response {
        session_id: Some(caller.identity.session_id.clone()),
        ..response(Status::Ready)
    };

    assert!(host.validate_issued_grant(&caller, &grant, &ready));
    let mut other_machine = grant.clone();
    other_machine.request.machine_id = "00000000-0000-4000-8000-000000000014".into();
    assert!(!host.validate_issued_grant(&caller, &other_machine, &ready));
    let mut wrong_destination = grant.clone();
    wrong_destination.request.approved_destination = Some("https://example.invalid".into());
    assert!(!host.validate_issued_grant(&caller, &wrong_destination, &ready));
    let mut wrong_network = grant.clone();
    wrong_network.request.network_profile =
        crate::computer_use::ComputerNetworkProfileV1::ApprovedInternet;
    assert!(!host.validate_issued_grant(&caller, &wrong_network, &ready));
    let mut smaller_budget = grant.clone();
    smaller_budget.request.resource_budget.cpu_millis = 1;
    assert!(!host.validate_issued_grant(&caller, &smaller_budget, &ready));
    let mut shorter_expiry = grant.clone();
    shorter_expiry.expires_at_unix_ms -= 1;
    assert!(!host.validate_issued_grant(&caller, &shorter_expiry, &ready));
    let mut later_approval = grant.clone();
    later_approval.approved_at_unix_ms += 1;
    assert!(!host.validate_issued_grant(&caller, &later_approval, &ready));
    let mut wrong_app = grant.clone();
    wrong_app.request.allowed_apps = vec!["text-editor".into()];
    assert!(!host.validate_issued_grant(&caller, &wrong_app, &ready));

    let overbroad = Arc::new(ComputerUseAuthorityRegistryV1::default());
    register_session(&overbroad, &caller, &grant, true);
    let overbroad_host = RegistryBoundComputerUseMcpHostV1::new(
        overbroad.clone(),
        ComputerUseConsentBrokerV1::default(),
        Arc::new(Operations(AtomicUsize::new(0))),
    );
    assert!(!overbroad_host.validate_issued_grant(&caller, &grant, &ready));
    overbroad.revoke(&caller.identity.session_id).unwrap();
    assert!(matches!(
        overbroad.authorize(
            &caller.identity,
            caller.actor,
            &caller.role_id,
            unix_time_ms()
        ),
        Err(crate::computer_use::guest_protocol::GuestProtocolErrorV1::StaleLease)
    ));
}
