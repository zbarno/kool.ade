//! Production boundary: public Rust serde values consumed as versioned JSON by
//! an independent controller/guest implementation. Unit validator tests cannot
//! prove stable wire shape or strict deserialization. No external services are
//! required; the fixture is synthetic and setup cost is low.

use koolade::computer_use::*;

const GOLDEN: &str = include_str!("fixtures/computer_use/contracts-v1.json");

#[test]
fn golden_wire_contracts_validate_and_round_trip_without_shape_drift() {
    let fixtures: serde_json::Value = serde_json::from_str(GOLDEN).unwrap();
    let identity: ComputerIdentityV1 = decode(&fixtures, "identity");
    let lease: ComputerLeaseV1 = decode(&fixtures, "lease");
    let policy: ComputerPolicyV1 = decode(&fixtures, "policy");
    let session: ComputerSessionV1 = decode(&fixtures, "session");
    let action: ComputerActionV1 = decode(&fixtures, "action");
    let observation: ComputerObservationV1 = decode(&fixtures, "observation");
    let event: ComputerEventV1 = decode(&fixtures, "event");

    assert_eq!(identity.validate(), Ok(()));
    assert_eq!(lease.validate(1_050_000), Ok(()));
    assert_eq!(policy.validate(), Ok(()));
    assert_eq!(session.validate(1_050_000), Ok(()));
    assert_eq!(action.validate(&session), Ok(()));
    assert_eq!(
        action.validate_as(ComputerActionActorV1::Agent, &session),
        Ok(())
    );
    let mut forged_actor = action.clone();
    forged_actor.actor = ComputerActionActorV1::Human;
    assert_eq!(
        forged_actor.validate_as(ComputerActionActorV1::Agent, &session),
        Err("action actor does not match authenticated caller")
    );
    assert_eq!(observation.validate(), Ok(()));
    assert_eq!(event.validate(), Ok(()));
    for (key, value) in [
        ("identity", serde_json::to_value(identity).unwrap()),
        ("lease", serde_json::to_value(lease).unwrap()),
        ("policy", serde_json::to_value(policy).unwrap()),
        ("session", serde_json::to_value(session).unwrap()),
        ("action", serde_json::to_value(action).unwrap()),
        ("observation", serde_json::to_value(observation).unwrap()),
        ("event", serde_json::to_value(event).unwrap()),
    ] {
        assert_eq!(value, fixtures[key], "wire shape changed for {key}");
    }
}

#[test]
fn wire_decoder_rejects_unknown_permissions_credentials_paths_and_versions() {
    let fixtures: serde_json::Value = serde_json::from_str(GOLDEN).unwrap();
    let session: ComputerSessionV1 = decode(&fixtures, "session");
    let mut action = serde_json::from_str::<serde_json::Value>(GOLDEN).unwrap()["action"].clone();
    action["host_path"] = serde_json::json!("/synthetic/host/path");
    assert!(serde_json::from_value::<ComputerActionV1>(action).is_err());

    let mut action = serde_json::from_str::<serde_json::Value>(GOLDEN).unwrap()["action"].clone();
    action["payload"]["host_path"] = serde_json::json!("/synthetic/host/path");
    assert!(serde_json::from_value::<ComputerActionV1>(action).is_err());

    let mut session_value =
        serde_json::from_str::<serde_json::Value>(GOLDEN).unwrap()["session"].clone();
    session_value["unreviewed_network_acl"] = serde_json::json!(["0.0.0.0/0"]);
    assert!(serde_json::from_value::<ComputerSessionV1>(session_value).is_err());

    let mut policy = serde_json::from_str::<serde_json::Value>(GOLDEN).unwrap()["policy"].clone();
    policy["provider_token"] = serde_json::json!("synthetic-secret");
    assert!(serde_json::from_value::<ComputerPolicyV1>(policy).is_err());

    let mut action = serde_json::from_str::<serde_json::Value>(GOLDEN).unwrap()["action"].clone();
    action["operation"] = serde_json::json!("arbitrary_host_shell");
    assert!(serde_json::from_value::<ComputerActionV1>(action).is_err());

    let mut action: ComputerActionV1 = decode(&serde_json::from_str(GOLDEN).unwrap(), "action");
    action.schema_version = 99;
    assert_eq!(action.validate(&session), Err("unsupported action version"));

    let mut payload = serde_json::from_str::<serde_json::Value>(GOLDEN).unwrap()["action"].clone();
    payload["operation"] = serde_json::json!("observe_screen");
    assert!(
        serde_json::from_value::<ComputerActionV1>(payload)
            .unwrap()
            .validate(&session)
            .is_err()
    );

    let mut launch = decode::<ComputerActionV1>(&serde_json::from_str(GOLDEN).unwrap(), "action");
    launch.operation = ComputerOperationV1::LaunchAllowedApp;
    launch.payload = ComputerActionPayloadV1::LaunchApp {
        app_id: "browser".into(),
    };
    assert_eq!(
        launch.validate(&session),
        Err("app is not in the session allowlist")
    );
}

#[test]
fn event_stream_fences_sequences_actors_and_uncertain_external_effects() {
    let fixtures: serde_json::Value = serde_json::from_str(GOLDEN).unwrap();
    let action: ComputerActionV1 = decode(&fixtures, "action");
    let session: ComputerSessionV1 = decode(&fixtures, "session");
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[42; 32]);
    let verifying_key = ed25519_dalek::VerifyingKey::from(&signing_key);
    let mut validator = ComputerEventValidator::new(
        &session,
        1_050_000,
        "00000000-0000-4000-8000-000000000011",
        &verifying_key,
    )
    .unwrap();
    validator
        .accept(&ComputerEventV1 {
            schema_version: 1,
            session_id: action.session_id.clone(),
            sequence: 1,
            occurred_at_unix_ms: 1000500,
            lease_generation: 1,
            approval_generation: 1,
            cancellation_generation: 1,
            kind: ComputerEventKindV1::StateChanged {
                from: ComputerLifecycleV1::Ready,
                to: ComputerLifecycleV1::Delegated,
            },
        })
        .unwrap();

    assert_eq!(
        validator.accept(&ComputerEventV1 {
            schema_version: 1,
            session_id: action.session_id.clone(),
            sequence: 2,
            occurred_at_unix_ms: 1000600,
            lease_generation: 1,
            approval_generation: 1,
            cancellation_generation: 1,
            kind: ComputerEventKindV1::LeaseRevoked {
                cancellation_generation: 1,
            },
        }),
        Err("lease revocation does not match cancellation fence")
    );
    let accepted = ComputerEventV1 {
        schema_version: 1,
        session_id: action.session_id.clone(),
        sequence: 2,
        occurred_at_unix_ms: 1001000,
        lease_generation: 1,
        approval_generation: 1,
        cancellation_generation: 1,
        kind: ComputerEventKindV1::ActionAccepted {
            action: action.clone(),
        },
    };
    validator.accept(&accepted).unwrap();
    assert_eq!(
        validator.accept(&accepted),
        Err("event sequence is replayed or discontinuous")
    );

    assert_eq!(
        validator.accept(&ComputerEventV1 {
            schema_version: 1,
            session_id: action.session_id.clone(),
            sequence: 3,
            occurred_at_unix_ms: 1001500,
            lease_generation: 1,
            approval_generation: 1,
            cancellation_generation: 1,
            kind: ComputerEventKindV1::StateChanged {
                from: ComputerLifecycleV1::Delegated,
                to: ComputerLifecycleV1::HumanControl,
            },
        }),
        Err("cannot change lifecycle while an action is unresolved")
    );

    validator
        .accept(&ComputerEventV1 {
            schema_version: 1,
            session_id: action.session_id.clone(),
            sequence: 3,
            occurred_at_unix_ms: 1002000,
            lease_generation: 1,
            approval_generation: 1,
            cancellation_generation: 1,
            kind: ComputerEventKindV1::ActionCompleted {
                action_sequence: 1,
                status: ComputerStatusV1::Uncertain,
                failure: Some(ComputerFailureCodeV1::UncertainSideEffect),
            },
        })
        .unwrap();

    let mut retry = action.clone();
    retry.action_sequence = 2;
    retry.idempotency_key = "00000000-0000-4000-8000-000000000010".into();
    let result = validator.accept(&ComputerEventV1 {
        schema_version: 1,
        session_id: retry.session_id.clone(),
        sequence: 4,
        occurred_at_unix_ms: 1003000,
        lease_generation: 1,
        approval_generation: 1,
        cancellation_generation: 1,
        kind: ComputerEventKindV1::ActionAccepted { action: retry },
    });
    assert_eq!(result, Err("action actor does not own computer control"));

    let mut renewed = session;
    renewed.lifecycle = ComputerLifecycleV1::Recovering;
    renewed.action_sequence = 1;
    renewed.approval_generation = 2;
    renewed.policy.approval_generation = 2;
    renewed.lease.generation = 2;
    renewed.lease.issued_at_unix_ms = 1_050_000;
    renewed.lease.expires_at_unix_ms = 1_900_000;
    renewed
        .attest("00000000-0000-4000-8000-000000000011".into(), &signing_key)
        .unwrap();
    validator
        .advance_authority(
            &renewed,
            1_050_000,
            "00000000-0000-4000-8000-000000000011",
            &verifying_key,
        )
        .unwrap();
    let mut cancelled = renewed;
    cancelled.lifecycle = ComputerLifecycleV1::Interrupted;
    cancelled.cancellation_generation = 2;
    cancelled.lease.cancellation_generation = 2;
    cancelled
        .attest("00000000-0000-4000-8000-000000000011".into(), &signing_key)
        .unwrap();
    validator
        .advance_authority(
            &cancelled,
            1_050_000,
            "00000000-0000-4000-8000-000000000011",
            &verifying_key,
        )
        .unwrap();
    validator
        .accept(&ComputerEventV1 {
            schema_version: 1,
            session_id: action.session_id.clone(),
            sequence: 4,
            occurred_at_unix_ms: 1003500,
            lease_generation: 2,
            approval_generation: 2,
            cancellation_generation: 2,
            kind: ComputerEventKindV1::LeaseRevoked {
                cancellation_generation: 2,
            },
        })
        .unwrap();
    let stale_lease = ComputerEventV1 {
        schema_version: 1,
        session_id: action.session_id.clone(),
        sequence: 5,
        occurred_at_unix_ms: 1004000,
        lease_generation: 1,
        approval_generation: 2,
        cancellation_generation: 2,
        kind: ComputerEventKindV1::StateChanged {
            from: ComputerLifecycleV1::Interrupted,
            to: ComputerLifecycleV1::Recovering,
        },
    };
    assert_eq!(
        validator.accept(&stale_lease),
        Err("event lease generation does not match controller state")
    );
    let stale_approval = ComputerEventV1 {
        lease_generation: 2,
        approval_generation: 1,
        cancellation_generation: 2,
        kind: ComputerEventKindV1::ActionAccepted {
            action: action.clone(),
        },
        ..stale_lease
    };
    assert_eq!(
        validator.accept(&stale_approval),
        Err("event authority generation does not match controller state")
    );
}

#[test]
fn controller_attestation_binds_identity_policy_lease_target_and_image() {
    let fixtures: serde_json::Value = serde_json::from_str(GOLDEN).unwrap();
    let mut session: ComputerSessionV1 = decode(&fixtures, "session");
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[42; 32]);
    let verifying_key = ed25519_dalek::VerifyingKey::from(&signing_key);
    let key_id = "00000000-0000-4000-8000-000000000011";
    session
        .verify_attestation(key_id, &verifying_key)
        .expect("golden fixture has a valid synthetic controller signature");
    session.attest(key_id.to_owned(), &signing_key).unwrap();
    session
        .validate_for_host(1_050_000, key_id, &verifying_key)
        .unwrap();

    session.image_digest = format!("sha256:{}", "c".repeat(64));
    assert_eq!(
        session.verify_attestation(key_id, &verifying_key),
        Err("controller session attestation is invalid")
    );
    assert_eq!(
        session.verify_attestation("00000000-0000-4000-8000-000000000012", &verifying_key),
        Err("controller signing key identity mismatch")
    );
}

fn decode<T: serde::de::DeserializeOwned>(fixtures: &serde_json::Value, key: &str) -> T {
    serde_json::from_value(fixtures[key].clone())
        .unwrap_or_else(|error| panic!("invalid {key} fixture: {error}"))
}
