use crate::{
    execution::docker::images::{ImageLock, RuntimeProfile},
    persistence::setup_tasks::{
        AuthorizationBindingV1, ImagePreparationReceiptV1, SetupTaskStateV1, SetupTaskTransitionV1,
        apply_transition_at_for_test, authorization_scope_digest, begin_scan_at_for_test,
        load_from_for_test, reconcile_at_if,
    },
};

const TASK_ID: &str = concat!(
    "setup:images:rust:amd64:",
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
);
const OPERATION_ID: &str = "00000000-0000-4000-8000-000000000301";
const AUTH_RECEIPT_ID: &str = "00000000-0000-4000-8000-000000000302";

#[test]
fn typed_image_receipt_persists_only_for_the_approved_attempt() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("machine-setup").join("tasks.json");
    let generation = begin_scan_at_for_test(&path).unwrap();
    let mut snapshot = super::support::snapshot(
        &[(
            TASK_ID,
            crate::app::setup_attention::catalog::FindingStatus::NeedsAttention,
            &[],
        )],
        &[],
        10,
    );
    snapshot.scan_generation = generation;
    assert!(
        reconcile_at_if(&path, &snapshot, generation, || true)
            .unwrap()
            .is_some()
    );

    apply_transition_at_for_test(
        &path,
        SetupTaskTransitionV1::RequestAttempt {
            task_id: TASK_ID.into(),
            operation_id: OPERATION_ID.into(),
            version: 1,
            requested_scopes: vec![TASK_ID.into()],
            at_unix_ms: 11,
        },
    )
    .unwrap();
    let scopes = vec![TASK_ID.to_owned()];
    apply_transition_at_for_test(
        &path,
        SetupTaskTransitionV1::RecordAuthorization {
            task_id: TASK_ID.into(),
            operation_id: OPERATION_ID.into(),
            binding: AuthorizationBindingV1 {
                receipt_id: AUTH_RECEIPT_ID.into(),
                scope_digest: authorization_scope_digest(TASK_ID, OPERATION_ID, 1, &scopes),
                authorized_at_unix_ms: 12,
            },
            at_unix_ms: 12,
        },
    )
    .unwrap();
    let selected = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::Rust, "amd64")
        .unwrap();
    apply_transition_at_for_test(
        &path,
        SetupTaskTransitionV1::RecordImagePreparationReceipt {
            task_id: TASK_ID.into(),
            operation_id: OPERATION_ID.into(),
            receipt: ImagePreparationReceiptV1 {
                profile: RuntimeProfile::Rust,
                architecture: "amd64".into(),
                image_digest: selected.digest,
                engine_identity_fingerprint: "b".repeat(64),
                successful_smoke_at_unix_ms: 20,
            },
            at_unix_ms: 20,
        },
    )
    .unwrap();
    let after_smoke = load_from_for_test(&path).unwrap().0;
    assert_eq!(
        after_smoke.tasks[TASK_ID].state,
        SetupTaskStateV1::InProgress
    );
    assert!(!after_smoke.has_valid_image_preparation_receipt(TASK_ID));

    apply_transition_at_for_test(
        &path,
        SetupTaskTransitionV1::AwaitVerification {
            task_id: TASK_ID.into(),
            operation_id: OPERATION_ID.into(),
            at_unix_ms: 21,
        },
    )
    .unwrap();
    let awaiting = load_from_for_test(&path).unwrap().0;
    assert_eq!(
        awaiting.tasks[TASK_ID].state,
        SetupTaskStateV1::AwaitingVerification
    );
    assert!(awaiting.has_valid_image_preparation_receipt(TASK_ID));

    let mut verified = awaiting;
    let domain = concat!(
        "images:rust:amd64:",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    );
    super::support::scan(&mut verified, &super::support::snapshot(&[], &[domain], 22)).unwrap();
    assert_eq!(verified.tasks[TASK_ID].state, SetupTaskStateV1::Verified);
    assert!(verified.has_valid_image_preparation_receipt(TASK_ID));
}
