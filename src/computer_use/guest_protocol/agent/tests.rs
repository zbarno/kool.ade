use super::*;
use crate::computer_use::guest_protocol::{GuestExitReasonV1, GuestOperationResultV1};
use crate::computer_use::{ComputerActionPayloadV1, ComputerActionV1};

#[test]
fn advertised_apps_are_the_intersection_of_config_and_guest_registry() {
    let config = GuestBootConfigV1 {
        schema_version: 1,
        session_id: "00000000-0000-4000-8000-000000000001".into(),
        worker_id: "00000000-0000-4000-8000-000000000002".into(),
        lease_generation: 1,
        initial_action_sequence: 0,
        expires_at_unix_ms: 60_000,
        boot_id: "00000000-0000-4000-8000-000000000003".into(),
        secret_hex: "11".repeat(32),
        allowed_apps: vec!["chromium".into(), "unregistered-app".into()],
    };
    let capabilities = guest_capabilities(&config);
    assert!(
        capabilities
            .app_registry
            .iter()
            .all(|app| app == "chromium")
    );
    assert!(!capabilities.accessibility_tree);
}

#[test]
fn accessibility_is_reported_unsupported_without_creating_side_effects() {
    let action = ComputerActionV1 {
        schema_version: 1,
        session_id: "00000000-0000-4000-8000-000000000001".into(),
        action_sequence: 1,
        idempotency_key: "00000000-0000-4000-8000-000000000002".into(),
        actor: crate::computer_use::ComputerActionActorV1::Agent,
        lease_generation: 1,
        approval_generation: 1,
        cancellation_generation: 1,
        operation: crate::computer_use::ComputerOperationV1::ReadAccessibility,
        payload: ComputerActionPayloadV1::ReadAccessibility,
    };
    let config = GuestBootConfigV1 {
        schema_version: 1,
        session_id: action.session_id.clone(),
        worker_id: "00000000-0000-4000-8000-000000000003".into(),
        lease_generation: 1,
        initial_action_sequence: 0,
        expires_at_unix_ms: 60_000,
        boot_id: "00000000-0000-4000-8000-000000000004".into(),
        secret_hex: "22".repeat(32),
        allowed_apps: vec![],
    };
    let result =
        action::execute_action(&action, &config, &mut AgentReplayState::default()).unwrap();
    assert_eq!(
        result.reply.result,
        GuestOperationResultV1::UnsupportedCapability
    );
    assert!(result.observation.is_none());
}

#[test]
fn guest_app_install_fails_closed_until_a_trusted_package_helper_exists() {
    let action = ComputerActionV1 {
        schema_version: 1,
        session_id: "00000000-0000-4000-8000-000000000001".into(),
        action_sequence: 1,
        idempotency_key: "00000000-0000-4000-8000-000000000002".into(),
        actor: crate::computer_use::ComputerActionActorV1::Human,
        lease_generation: 1,
        approval_generation: 1,
        cancellation_generation: 1,
        operation: crate::computer_use::ComputerOperationV1::ManageApp,
        payload: ComputerActionPayloadV1::ManageApp {
            app_id: "text-editor".into(),
            operation: crate::computer_use::apps::GuestAppOperationKindV1::Install,
            catalog_digest: format!("sha256:{}", "c".repeat(64)),
            image_digest: format!("sha256:{}", "d".repeat(64)),
            manifest_sha256: format!("sha256:{}", "a".repeat(64)),
            package_sha256: format!("sha256:{}", "b".repeat(64)),
        },
    };
    let config = GuestBootConfigV1 {
        schema_version: 1,
        session_id: action.session_id.clone(),
        worker_id: "00000000-0000-4000-8000-000000000003".into(),
        lease_generation: 1,
        initial_action_sequence: 0,
        expires_at_unix_ms: 60_000,
        boot_id: "00000000-0000-4000-8000-000000000004".into(),
        secret_hex: "22".repeat(32),
        allowed_apps: vec!["text-editor".into()],
    };
    let result =
        action::execute_action(&action, &config, &mut AgentReplayState::default()).unwrap();
    assert_eq!(
        result.reply.result,
        GuestOperationResultV1::UnsupportedCapability
    );
    assert!(result.observation.is_none());
}

#[test]
fn action_failures_emit_authenticated_structured_exit_reason() {
    let config = GuestBootConfigV1 {
        schema_version: 1,
        session_id: "00000000-0000-4000-8000-000000000001".into(),
        worker_id: "00000000-0000-4000-8000-000000000002".into(),
        lease_generation: 1,
        initial_action_sequence: 0,
        expires_at_unix_ms: 60_000,
        boot_id: "00000000-0000-4000-8000-000000000003".into(),
        secret_hex: "33".repeat(32),
        allowed_apps: vec![],
    };
    let handshake = GuestHandshakeCompleteV1 {
        channel_key: crate::computer_use::guest_protocol::GuestChannelKeyV1::from_bytes([7; 32]),
        capabilities: guest_capabilities(&config),
        transcript_sha256: "a".repeat(64),
        boot_id: config.boot_id.clone(),
        request_id: "00000000-0000-4000-8000-000000000004".into(),
        action_id: "00000000-0000-4000-8000-000000000005".into(),
        host_sequence: 3,
        guest_sequence: 3,
    };
    let request = envelope(
        &config,
        crate::computer_use::guest_protocol::types::GuestEnvelopeContextV1 {
            sender: GuestSenderV1::Host,
            sequence: 3,
            request_id: handshake.request_id.clone(),
            action_id: handshake.action_id.clone(),
            deadline_unix_ms: 50_000,
            capability_id: "computer_use.screen.v1".to_owned(),
        },
        GuestMessageV1::Heartbeat {
            boot_id: config.boot_id.clone(),
            uptime_millis: 1,
        },
    );
    let mut output = std::io::Cursor::new(Vec::new());
    write_exit_notice(
        &mut output,
        &config,
        &handshake,
        3,
        &request,
        GuestProtocolErrorV1::Io,
    )
    .unwrap();
    output.set_position(0);
    let signed: AuthenticatedGuestEnvelopeV1 = read_frame(&mut output).unwrap();
    signed.verify(handshake.channel_key.expose()).unwrap();
    assert_eq!(
        signed.envelope.message,
        GuestMessageV1::GuestExit {
            reason: GuestExitReasonV1::GuestInteractionFailed
        }
    );
}
