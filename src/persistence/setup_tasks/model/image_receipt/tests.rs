use crate::{
    app::setup_attention::catalog::{Finding, FindingStatus},
    execution::docker::images::{ImageLock, RuntimeProfile},
    persistence::setup_tasks::{
        AttemptState, AuthorizationBindingV1, ImagePreparationReceiptV1, SetupAttemptV1,
        SetupTaskEventV1, SetupTaskStateV1, SetupTaskStoreV1, SetupTaskV1,
        authorization_scope_digest,
    },
};

const TASK_ID: &str = concat!(
    "setup:images:general:amd64:",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
);
const OPERATION_ID: &str = "00000000-0000-4000-8000-000000000201";
const AUTH_RECEIPT_ID: &str = "00000000-0000-4000-8000-000000000202";
const OWNER_ID: &str = "00000000-0000-4000-8000-000000000203";

fn prepared_store() -> SetupTaskStoreV1 {
    let finding = Finding {
        id: TASK_ID.into(),
        title: "Prepare the approved image".into(),
        explanation: "A synthetic image setup task.".into(),
        next_step: "Review and prepare the image.".into(),
        status: FindingStatus::NeedsAttention,
        prerequisites: Vec::new(),
    };
    let mut task = SetupTaskV1::from_finding(&finding, 1, 10);
    task.state = SetupTaskStateV1::ReadyToFix;
    let mut store = SetupTaskStoreV1 {
        latest_scan_generation: 1,
        ..SetupTaskStoreV1::default()
    };
    store.tasks.insert(TASK_ID.into(), task);
    store
}

fn begin_owned_attempt(store: &mut SetupTaskStoreV1) {
    store
        .request_attempt(
            TASK_ID,
            OPERATION_ID.into(),
            1,
            vec![TASK_ID.into()],
            OWNER_ID,
            11,
        )
        .unwrap();
    let scopes = vec![TASK_ID.to_owned()];
    store
        .authorize_attempt(
            TASK_ID,
            OPERATION_ID,
            AuthorizationBindingV1 {
                receipt_id: AUTH_RECEIPT_ID.into(),
                scope_digest: authorization_scope_digest(TASK_ID, OPERATION_ID, 1, &scopes),
                authorized_at_unix_ms: 12,
            },
            OWNER_ID,
            12,
        )
        .unwrap();
}

fn receipt(at: u64) -> ImagePreparationReceiptV1 {
    let selected = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    ImagePreparationReceiptV1 {
        profile: RuntimeProfile::General,
        architecture: "amd64".into(),
        image_digest: selected.digest,
        engine_identity_fingerprint: "a".repeat(64),
        successful_smoke_at_unix_ms: at,
    }
}

#[test]
fn image_receipt_requires_an_owned_in_progress_attempt_and_current_locked_identity() {
    let mut store = prepared_store();
    begin_owned_attempt(&mut store);
    let mut bad_engine = receipt(20);
    bad_engine.engine_identity_fingerprint = "b".repeat(64);
    assert!(
        store
            .record_image_preparation_receipt(TASK_ID, OPERATION_ID, OWNER_ID, bad_engine, 20)
            .is_err()
    );

    let mut bad_digest = receipt(20);
    bad_digest.image_digest = format!("sha256:{}", "f".repeat(64));
    assert!(
        store
            .record_image_preparation_receipt(TASK_ID, OPERATION_ID, OWNER_ID, bad_digest, 20)
            .is_err()
    );

    let good = receipt(20);
    assert!(
        store
            .record_image_preparation_receipt(
                TASK_ID,
                OPERATION_ID,
                "wrong-owner",
                good.clone(),
                20
            )
            .is_err()
    );
    store
        .record_image_preparation_receipt(TASK_ID, OPERATION_ID, OWNER_ID, good, 20)
        .unwrap();
    assert!(!store.has_valid_image_preparation_receipt(TASK_ID));

    store
        .await_verification(TASK_ID, OPERATION_ID, OWNER_ID, 21)
        .unwrap();
    assert!(store.has_valid_image_preparation_receipt(TASK_ID));
    store.validate().unwrap();
}

#[test]
fn image_receipt_serde_uses_bounded_typed_attempt_data() {
    let mut store = prepared_store();
    begin_owned_attempt(&mut store);
    let good = receipt(20);
    store
        .record_image_preparation_receipt(TASK_ID, OPERATION_ID, OWNER_ID, good, 20)
        .unwrap();
    let encoded = serde_json::to_vec(&store).unwrap();
    let decoded: SetupTaskStoreV1 = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, store);

    let attempt: &SetupAttemptV1 = &decoded.tasks[TASK_ID].attempts[0];
    assert_eq!(attempt.state, AttemptState::InProgress);
    assert_eq!(
        attempt
            .image_preparation_receipt
            .as_ref()
            .unwrap()
            .successful_smoke_at_unix_ms,
        20
    );
}

#[test]
fn legacy_attempt_without_image_receipt_deserializes_empty() {
    let mut store = prepared_store();
    begin_owned_attempt(&mut store);
    store
        .tasks
        .get_mut(TASK_ID)
        .unwrap()
        .events
        .push(SetupTaskEventV1 {
            observed_at_unix_ms: 12,
            scan_generation: 1,
            state: SetupTaskStateV1::InProgress,
        });
    let mut value = serde_json::to_value(store).unwrap();
    value["tasks"][TASK_ID]["attempts"][0]
        .as_object_mut()
        .unwrap()
        .remove("imagePreparationReceipt");
    let decoded: SetupTaskStoreV1 = serde_json::from_value(value).unwrap();
    assert!(
        decoded.tasks[TASK_ID].attempts[0]
            .image_preparation_receipt
            .is_none()
    );
}
