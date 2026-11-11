use super::*;

#[test]
fn personal_session_takeover_and_handback_replace_guest_authority() {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[41; 32]);
    let verifying_key = signing_key.verifying_key();
    let key_id = "00000000-0000-4000-8000-000000000008";
    let registry = ComputerUseAuthorityRegistryV1::default();
    let mut delegated = personal_session();
    delegated.lifecycle = ComputerLifecycleV1::Delegated;
    delegated.attest(key_id.into(), &signing_key).unwrap();
    let identity = delegated.identity.clone();
    registry
        .register(
            delegated.clone(),
            key_id.into(),
            verifying_key,
            ComputerActionActorV1::Agent,
            "implementer/v1".into(),
            NOW,
        )
        .unwrap();
    let stale_agent = registry
        .authorize(
            &identity,
            ComputerActionActorV1::Agent,
            "implementer/v1",
            NOW,
        )
        .unwrap();

    let mut human_control = delegated.clone();
    human_control.lifecycle = ComputerLifecycleV1::HumanControl;
    human_control.attest(key_id.into(), &signing_key).unwrap();
    registry
        .replace_actor(
            human_control.clone(),
            key_id.into(),
            verifying_key,
            ComputerActionActorV1::Human,
            "implementer/v1".into(),
            NOW,
        )
        .unwrap();
    assert_eq!(
        stale_agent.validate_call(NOW),
        Err(GuestProtocolErrorV1::StaleLease)
    );
    assert!(
        registry
            .authorize(
                &identity,
                ComputerActionActorV1::Agent,
                "implementer/v1",
                NOW,
            )
            .is_err()
    );
    registry
        .authorize(
            &identity,
            ComputerActionActorV1::Human,
            "implementer/v1",
            NOW,
        )
        .unwrap();

    human_control.lifecycle = ComputerLifecycleV1::Delegated;
    human_control.attest(key_id.into(), &signing_key).unwrap();
    registry
        .replace_actor(
            human_control,
            key_id.into(),
            verifying_key,
            ComputerActionActorV1::Agent,
            "implementer/v1".into(),
            NOW,
        )
        .unwrap();
    assert!(
        registry
            .authorize(
                &identity,
                ComputerActionActorV1::Human,
                "implementer/v1",
                NOW,
            )
            .is_err()
    );
    registry
        .authorize(
            &identity,
            ComputerActionActorV1::Agent,
            "implementer/v1",
            NOW,
        )
        .unwrap();
}

fn personal_session() -> ComputerSessionV1 {
    let mut session = session();
    session.session_class = ComputerSessionClassV1::PersonalPersistent;
    session.policy.session_class = ComputerSessionClassV1::PersonalPersistent;
    session.policy.capability = ComputerCapabilityV1::PersistentPersonal;
    session.policy.authorized_operations.extend([
        ComputerOperationV1::LaunchAllowedApp,
        ComputerOperationV1::MouseInput,
        ComputerOperationV1::KeyboardInput,
    ]);
    session.policy.validate().unwrap();
    session.validate(NOW).unwrap();
    session
}
