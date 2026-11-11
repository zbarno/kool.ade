use super::common::*;

#[test]
fn approved_request_becomes_a_fenced_guest_action_with_shared_idempotency_key() {
    let service = provisioner();
    let request = request(identity(), Kind::Install);
    let action = service
        .computer_action(&request, &approval(&request), 7, 3)
        .unwrap();
    assert_eq!(action.idempotency_key, request.operation_id);
    assert_eq!(action.session_id, request.identity.session_id);
    assert_eq!(action.action_sequence, 7);
    assert_eq!(action.lease_generation, request.lease_generation);
    assert_eq!(action.approval_generation, request.approval_generation);
    assert_eq!(action.cancellation_generation, 3);
    assert_eq!(
        action.operation,
        crate::computer_use::ComputerOperationV1::ManageApp
    );
    assert!(matches!(
        action.payload,
        crate::computer_use::ComputerActionPayloadV1::ManageApp {
            app_id,
            manifest_sha256,
            package_sha256,
            ..
        } if app_id == request.app_id
            && manifest_sha256.starts_with("sha256:")
            && package_sha256 == APP_PACKAGE
    ));
}

#[test]
fn action_builder_rejects_stale_or_expanded_authority() {
    let service = provisioner();
    let request = request(identity(), Kind::Install);
    let approval_grant = approval(&request);
    let mut request_with_wrong_lease = request.clone();
    request_with_wrong_lease.lease_generation += 1;
    assert!(
        service
            .computer_action(&request_with_wrong_lease, &approval_grant, 1, 1)
            .is_err()
    );

    let mut request_with_extra_capability = request;
    request_with_extra_capability
        .granted_capabilities
        .insert(Capability::GuestNetwork);
    assert!(
        service
            .computer_action(&request_with_extra_capability, &approval_grant, 1, 1)
            .is_err()
    );
}

#[test]
fn action_builder_rejects_unlisted_apps_architecture_and_expired_consent() {
    let service = provisioner();
    let request = request(identity(), Kind::Install);
    let mut unknown = request.clone();
    unknown.app_id = "unknown-app".into();
    let unknown_approval = approval(&unknown);
    assert!(
        service
            .computer_action(&unknown, &unknown_approval, 1, 1)
            .is_err()
    );

    let mut wrong_arch = request.clone();
    wrong_arch.architecture = GuestAppArchitectureV1::Aarch64;
    let wrong_arch_approval = approval(&wrong_arch);
    assert!(
        service
            .computer_action(&wrong_arch, &wrong_arch_approval, 1, 1)
            .is_err()
    );

    let mut expired_approval = approval(&request);
    expired_approval.expires_at_unix_ms = request.now_unix_ms;
    assert!(
        service
            .computer_action(&request, &expired_approval, 1, 1)
            .is_err()
    );
}
