use crate::persistence::setup_tasks::{
    AuthorizationBindingV1, SetupTaskStoreV1, SetupTaskTransitionV1,
};

use super::super::AttemptContext;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PipelineFailureKind {
    AuthorizationPersistence,
    AuthorizationNotPublished,
    OperationFailed,
    AwaitVerificationPersistence,
}

pub(super) struct PipelineFailure {
    pub(super) kind: PipelineFailureKind,
    pub(super) store_update: Option<SetupTaskStoreV1>,
}

/// Shared production seam: durable authorization, UI publication, execution,
/// then a verification wait state. A failed stage prevents every later stage.
pub(super) fn authorize_publish_execute(
    attempt: &AttemptContext,
    binding: AuthorizationBindingV1,
    mut persist: impl FnMut(SetupTaskTransitionV1) -> anyhow::Result<SetupTaskStoreV1>,
    publish: impl FnOnce(SetupTaskStoreV1) -> bool,
    execute: impl FnOnce() -> Result<(), ()>,
) -> Result<SetupTaskStoreV1, PipelineFailure> {
    let authorized = persist(SetupTaskTransitionV1::RecordAuthorization {
        task_id: attempt.task_id.clone(),
        operation_id: attempt.operation_id.clone(),
        binding,
        at_unix_ms: now_ms(),
    })
    .map_err(|_| PipelineFailure {
        kind: PipelineFailureKind::AuthorizationPersistence,
        store_update: None,
    })?;
    if !publish(authorized.clone()) {
        return Err(PipelineFailure {
            kind: PipelineFailureKind::AuthorizationNotPublished,
            store_update: Some(authorized),
        });
    }
    execute().map_err(|_| PipelineFailure {
        kind: PipelineFailureKind::OperationFailed,
        store_update: Some(authorized.clone()),
    })?;
    persist(SetupTaskTransitionV1::AwaitVerification {
        task_id: attempt.task_id.clone(),
        operation_id: attempt.operation_id.clone(),
        at_unix_ms: now_ms(),
    })
    .map_err(|_| PipelineFailure {
        kind: PipelineFailureKind::AwaitVerificationPersistence,
        store_update: Some(authorized),
    })
}

fn now_ms() -> u64 {
    crate::persistence::harness_settings::current_unix_ms()
}
