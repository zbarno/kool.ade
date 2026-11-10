use super::*;
use crate::computer_use::{
    ComputerCapabilityV1, ComputerControllerProofV1, ComputerIdentityV1, ComputerLeaseV1,
    ComputerNetworkProfileV1, ComputerPolicyV1, ComputerResourceBudgetV1, ComputerSessionClassV1,
    ComputerSessionV1, ComputerTargetV1,
};
use sha2::Digest;
use std::collections::BTreeSet;

#[test]
fn action_authority_checks_actor_lifecycle_generations_and_budget() {
    let session = session();
    let action = action();
    assert_eq!(
        action.validate_as(ComputerActionActorV1::Agent, &session, 2_000),
        Ok(())
    );

    assert_eq!(
        action.validate_as(ComputerActionActorV1::Human, &session, 2_000),
        Err("action actor does not match authenticated caller")
    );
    let mut wrong_lifecycle = session.clone();
    wrong_lifecycle.lifecycle = super::super::super::ComputerLifecycleV1::HumanControl;
    assert_eq!(
        action.validate_as(ComputerActionActorV1::Agent, &wrong_lifecycle, 2_000),
        Err("action actor does not own computer control")
    );
    let mut expired = session.clone();
    expired.policy.resource_budget.time_limit_ms = 10_000;
    assert_eq!(
        action.validate(&expired, 11_000),
        Err("session action budget has expired")
    );
    let mut revoked = session;
    revoked.cancellation_generation += 1;
    assert_eq!(
        action.validate(&revoked, 2_000),
        Err("action scope or approval generation mismatch")
    );
}

#[test]
fn pointer_input_rejects_stale_geometry_and_excessive_scroll() {
    let session = session();
    let mut action = action();
    action.operation = ComputerOperationV1::MouseInput;
    let target = ComputerFrameReferenceV1 {
        frame_id: "00000000-0000-4000-8000-000000000005".into(),
        display_generation: 1,
        observation_revision: 1,
        display_width_px: 800,
        display_height_px: 600,
        device_scale_numerator: 1,
        device_scale_denominator: 1,
        window_id: "00000000-0000-4000-8000-000000000006".into(),
        window_generation: 1,
        window_bounds: ComputerWindowBoundsV1 {
            left: 0,
            top: 0,
            width_px: 800,
            height_px: 600,
        },
    };
    action.payload = ComputerActionPayloadV1::MouseClick {
        target: target.clone(),
        x: 800,
        y: 10,
        button: MouseButtonV1::Primary,
    };
    assert_eq!(
        action.validate(&session, 2_000),
        Err("pointer action does not match its observed frame and window")
    );
    action.payload = ComputerActionPayloadV1::MouseScroll {
        target,
        x: 10,
        y: 10,
        delta_x: 0,
        delta_y: 31,
    };
    assert_eq!(
        action.validate(&session, 2_000),
        Err("scroll delta exceeds bounds")
    );
}

#[test]
fn lease_renewal_does_not_reset_the_session_time_budget() {
    let mut session = session();
    let mut action = action();
    session.policy.resource_budget.time_limit_ms = 6_000;
    session.lease.issued_at_unix_ms = 5_000;
    session.lease.generation = 3;
    action.lease_generation = 3;
    assert_eq!(
        action.validate(&session, 7_000),
        Err("session action budget has expired")
    );
}

#[test]
fn action_authority_allows_completed_sequence_replay_for_the_durable_journal() {
    let mut session = session();
    let action = action();
    session.action_sequence = action.action_sequence;
    assert_eq!(
        action.validate_as(ComputerActionActorV1::Agent, &session, 2_000),
        Ok(())
    );
}

#[test]
fn guest_app_operations_require_human_control_and_exact_catalog_digests() {
    let mut session = session();
    session.lifecycle = super::super::super::ComputerLifecycleV1::HumanControl;
    session
        .policy
        .authorized_operations
        .insert(ComputerOperationV1::ManageApp);
    session.policy.allowed_apps.insert("chromium".into());
    session.allowed_apps.insert("chromium".into());
    let mut action = action();
    action.actor = ComputerActionActorV1::Human;
    action.operation = ComputerOperationV1::ManageApp;
    action.payload = ComputerActionPayloadV1::ManageApp {
        app_id: "chromium".into(),
        operation: crate::computer_use::apps::GuestAppOperationKindV1::Update,
        catalog_digest: format!("sha256:{}", "c".repeat(64)),
        image_digest: format!("sha256:{}", "d".repeat(64)),
        manifest_sha256: format!("sha256:{}", "a".repeat(64)),
        package_sha256: format!("sha256:{}", "b".repeat(64)),
    };
    assert_eq!(
        action.validate_as(ComputerActionActorV1::Human, &session, 2_000),
        Ok(())
    );
    assert!(
        action
            .validate_as(ComputerActionActorV1::Agent, &session, 2_000)
            .is_err()
    );

    if let ComputerActionPayloadV1::ManageApp {
        manifest_sha256, ..
    } = &mut action.payload
    {
        *manifest_sha256 = "file:///etc/passwd".into();
    }
    assert_eq!(
        action.validate(&session, 2_000),
        Err("guest app operation identity is invalid")
    );
}

#[test]
fn staged_task_launch_requires_human_control_and_session_bound_receipts() {
    let mut session = session();
    session.lifecycle = super::super::super::ComputerLifecycleV1::HumanControl;
    session.image_digest = format!("sha256:{}", "d".repeat(64));
    session
        .policy
        .allowed_apps
        .insert(crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_APP_ID_V1.into());
    session
        .allowed_apps
        .insert(crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_APP_ID_V1.into());
    session
        .policy
        .authorized_operations
        .insert(ComputerOperationV1::LaunchApprovedStagedTaskApp);
    let receipt = crate::computer_use::artifacts::ComputerArtifactReceiptV1 {
        artifact_id: "00000000-0000-4000-8000-000000000011".into(),
        display_name: "editor.txt".into(),
        size_bytes: 8,
        sha256: format!("sha256:{}", "1".repeat(64)),
        media_type: "text/plain".into(),
        expires_at_unix_ms: 30_000,
    };
    let artifacts = vec![crate::computer_use::apps::StagedTaskArtifactV1 {
        relative_path: "bin/editor.txt".into(),
        transfer_grant_id: "00000000-0000-4000-8000-000000000012".into(),
        receipt,
    }];
    let manifest = vec![("bin/editor.txt", "1".repeat(64))];
    let build_receipt_sha256 = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&manifest).unwrap())
    );
    let application = crate::computer_use::apps::StagedTaskApplicationV1 {
        schema_version: 1,
        app_id: crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_APP_ID_V1.into(),
        executable_id: "editor".into(),
        executable_relative_path: "bin/editor.txt".into(),
        task_revision: "revision-1".into(),
        app_target_id: "target-1".into(),
        execution_recipe_id: "recipe-1".into(),
        build_receipt_sha256,
        build_approval_sha256: "c".repeat(64),
        guest_image_digest: session.image_digest.clone(),
        guest_architecture: crate::computer_use::apps::GuestAppArchitectureV1::X86_64,
        artifacts,
    };
    let mut action = action();
    action.actor = ComputerActionActorV1::Human;
    action.operation = ComputerOperationV1::LaunchApprovedStagedTaskApp;
    action.payload = ComputerActionPayloadV1::LaunchApprovedStagedTaskApp {
        application: Box::new(application),
    };
    assert_eq!(
        action.validate_as(ComputerActionActorV1::Human, &session, 2_000),
        Ok(())
    );
    assert!(
        action
            .validate_as(ComputerActionActorV1::Agent, &session, 2_000)
            .is_err()
    );
    session.image_digest = format!("sha256:{}", "e".repeat(64));
    assert_eq!(
        action.validate(&session, 2_000),
        Err("staged task application is invalid or outside session approval")
    );
}

fn action() -> ComputerActionV1 {
    ComputerActionV1 {
        schema_version: 1,
        session_id: "00000000-0000-4000-8000-000000000004".into(),
        action_sequence: 1,
        idempotency_key: "00000000-0000-4000-8000-000000000007".into(),
        actor: ComputerActionActorV1::Agent,
        lease_generation: 2,
        approval_generation: 4,
        cancellation_generation: 3,
        operation: ComputerOperationV1::ObserveScreen,
        payload: ComputerActionPayloadV1::ObserveScreen,
        helper_authorization: None,
    }
}

#[test]
fn guest_helper_authorization_binds_exact_action_and_nonce() {
    let signer = ed25519_dalek::SigningKey::from_bytes(&[31; 32]);
    let wrong_signer = ed25519_dalek::SigningKey::from_bytes(&[32; 32]);
    let key_id = "00000000-0000-4000-8000-000000000008";
    let mut signed = action();
    signed
        .sign_for_guest_helper(
            key_id,
            super::super::super::ComputerActionHelperPhaseV1::Stage,
            &signer,
        )
        .unwrap();
    assert!(signed.verify_guest_helper_authorization(
        key_id,
        super::super::super::ComputerActionHelperPhaseV1::Stage,
        &signer.verifying_key()
    ));
    assert!(!signed.verify_guest_helper_authorization(
        key_id,
        super::super::super::ComputerActionHelperPhaseV1::Stage,
        &wrong_signer.verifying_key()
    ));
    assert!(!signed.verify_guest_helper_authorization(
        "00000000-0000-4000-8000-000000000009",
        super::super::super::ComputerActionHelperPhaseV1::Stage,
        &signer.verifying_key()
    ));
    assert!(!signed.verify_guest_helper_authorization(
        key_id,
        super::super::super::ComputerActionHelperPhaseV1::Launch,
        &signer.verifying_key()
    ));
    let mut changed = signed;
    changed.action_sequence += 1;
    assert!(!changed.verify_guest_helper_authorization(
        key_id,
        super::super::super::ComputerActionHelperPhaseV1::Stage,
        &signer.verifying_key()
    ));
}

fn session() -> ComputerSessionV1 {
    let identity = ComputerIdentityV1 {
        project_id: "00000000-0000-4000-8000-000000000001".into(),
        task_id: "00000000-0000-4000-8000-000000000002".into(),
        user_id: "00000000-0000-4000-8000-000000000003".into(),
        session_id: "00000000-0000-4000-8000-000000000004".into(),
    };
    let allowed_apps = BTreeSet::from(["chromium".into()]);
    let authorized_operations = BTreeSet::from([
        ComputerOperationV1::ObserveScreen,
        ComputerOperationV1::MouseInput,
    ]);
    let policy = ComputerPolicyV1 {
        schema_version: 1,
        identity: identity.clone(),
        session_class: ComputerSessionClassV1::Disposable,
        capability: ComputerCapabilityV1::ApprovedGuiTask,
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps: allowed_apps.clone(),
        authorized_operations,
        approval_generation: 4,
        resource_budget: ComputerResourceBudgetV1 {
            time_limit_ms: 600_000,
            ..crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1
        },
    };
    ComputerSessionV1 {
        schema_version: 1,
        session_started_at_unix_ms: 1_000,
        identity,
        session_class: ComputerSessionClassV1::Disposable,
        target: ComputerTargetV1::LocalKvm {
            machine_id: "00000000-0000-4000-8000-000000000008".into(),
        },
        image_digest: format!("sha256:{}", "a".repeat(64)),
        image_manifest_digest: format!("sha256:{}", "b".repeat(64)),
        image_signer_key_id: "00000000-0000-4000-8000-000000000009".into(),
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps,
        policy,
        lease: ComputerLeaseV1 {
            schema_version: 1,
            owner_id: "00000000-0000-4000-8000-000000000003".into(),
            lease_id: "00000000-0000-4000-8000-000000000010".into(),
            generation: 2,
            issued_at_unix_ms: 1_000,
            expires_at_unix_ms: 900_000,
            cancellation_generation: 3,
        },
        lifecycle: super::super::super::ComputerLifecycleV1::Delegated,
        action_sequence: 0,
        approval_generation: 4,
        cancellation_generation: 3,
        evidence_refs: vec![],
        controller_proof: ComputerControllerProofV1 {
            signer_key_id: "00000000-0000-4000-8000-000000000011".into(),
            signature_hex: "a".repeat(128),
        },
    }
}
