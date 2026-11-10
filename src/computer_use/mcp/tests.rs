use super::{ComputerUseMcpRequestV1 as Request, validate_request};
use crate::computer_use::{
    ComputerActionPayloadV1 as Payload, apps::GuestAppOperationKindV1 as AppOperation,
};

fn manage_request(idempotency_key: Option<String>) -> Request {
    Request::Action {
        session_id: "00000000-0000-4000-8000-000000000001".into(),
        payload: Payload::ManageApp {
            app_id: "text-editor".into(),
            operation: AppOperation::Install,
            catalog_digest:
                "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into(),
            image_digest: "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
                .into(),
            manifest_sha256:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            package_sha256:
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        },
        timeout_ms: 60_000,
        idempotency_key,
    }
}

#[test]
fn managed_app_action_requires_a_canonical_controller_operation_id() {
    let operation_id = "00000000-0000-4000-8000-000000000002".to_owned();
    assert!(validate_request(&manage_request(Some(operation_id))).is_ok());
    assert!(validate_request(&manage_request(None)).is_err());
    assert!(validate_request(&manage_request(Some("not-a-uuid".into()))).is_err());
}

#[test]
fn other_actions_cannot_override_the_controller_idempotency_key() {
    let request = Request::Action {
        session_id: "00000000-0000-4000-8000-000000000001".into(),
        payload: Payload::QueryState,
        timeout_ms: 1_000,
        idempotency_key: Some("00000000-0000-4000-8000-000000000002".into()),
    };
    assert!(validate_request(&request).is_err());
}
