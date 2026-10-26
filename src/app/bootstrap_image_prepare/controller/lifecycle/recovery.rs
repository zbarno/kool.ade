use crate::persistence::setup_tasks::{
    AttemptState, FailureCode, SetupTaskStoreV1, SetupTaskTransitionV1,
};

use super::super::AttemptContext;
use super::RecoveryAction;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app::bootstrap_image_prepare::controller) enum AttemptResolution {
    Settled,
    Active(RecoveryAction),
}

pub(super) struct SettledAttempt {
    pub(super) store_update: Option<SetupTaskStoreV1>,
    pub(super) resolution: AttemptResolution,
}

pub(super) fn settle_failed_attempt(
    attempt: &AttemptContext,
    failure: FailureCode,
    apply: impl FnMut(SetupTaskTransitionV1) -> anyhow::Result<SetupTaskStoreV1>,
    load: impl FnMut() -> anyhow::Result<SetupTaskStoreV1>,
) -> SettledAttempt {
    settle_attempt_transition(attempt, RecoveryAction::Fail(failure), apply, load)
}

pub(super) fn settle_attempt_transition(
    attempt: &AttemptContext,
    action: RecoveryAction,
    mut apply: impl FnMut(SetupTaskTransitionV1) -> anyhow::Result<SetupTaskStoreV1>,
    mut load: impl FnMut() -> anyhow::Result<SetupTaskStoreV1>,
) -> SettledAttempt {
    if let Ok(store) = apply(transition(attempt, action)) {
        return settled_from(store, attempt, action);
    }

    let Ok(observed) = load() else {
        return active_without_snapshot(action);
    };
    let observed_state = attempt_state(&observed, attempt);
    if !is_active(observed_state) {
        return SettledAttempt {
            store_update: Some(observed),
            resolution: AttemptResolution::Settled,
        };
    }

    match apply(transition(attempt, action)) {
        Ok(store) => settled_from(store, attempt, action),
        Err(_) => match load() {
            Ok(store) => settled_from(store, attempt, action),
            Err(_) => SettledAttempt {
                store_update: Some(observed),
                resolution: AttemptResolution::Active(action),
            },
        },
    }
}

/// An AwaitVerification write can report an unlock error after the atomic
/// rename already committed. Reload before converting the successful repair
/// into a failure so the controller does not strand or mislabel it.
pub(super) fn recover_await_verification_write(
    attempt: &AttemptContext,
    mut load: impl FnMut() -> anyhow::Result<SetupTaskStoreV1>,
    apply: impl FnMut(SetupTaskTransitionV1) -> anyhow::Result<SetupTaskStoreV1>,
) -> (bool, SettledAttempt) {
    let store = match load() {
        Ok(store) => store,
        Err(_) => {
            return (
                false,
                active_without_snapshot(RecoveryAction::ReconcileVerification),
            );
        }
    };
    let state = attempt_state(&store, attempt);
    if matches!(
        state,
        Some(AttemptState::AwaitingVerification | AttemptState::Verified)
    ) {
        return (
            true,
            SettledAttempt {
                store_update: Some(store),
                resolution: AttemptResolution::Settled,
            },
        );
    }
    if !is_active(state) {
        return (
            state == Some(AttemptState::Verified),
            SettledAttempt {
                store_update: Some(store),
                resolution: AttemptResolution::Settled,
            },
        );
    }
    let settled = settle_failed_attempt(attempt, FailureCode::Unknown, apply, load);
    (false, settled)
}

fn settled_from(
    store: SetupTaskStoreV1,
    attempt: &AttemptContext,
    action: RecoveryAction,
) -> SettledAttempt {
    let state = attempt_state(&store, attempt);
    let resolution = if is_active(state) {
        AttemptResolution::Active(action)
    } else {
        AttemptResolution::Settled
    };
    SettledAttempt {
        store_update: Some(store),
        resolution,
    }
}

fn active_without_snapshot(action: RecoveryAction) -> SettledAttempt {
    SettledAttempt {
        store_update: None,
        resolution: AttemptResolution::Active(action),
    }
}

fn attempt_state(store: &SetupTaskStoreV1, context: &AttemptContext) -> Option<AttemptState> {
    store
        .tasks
        .get(&context.task_id)?
        .attempts
        .iter()
        .find(|attempt| attempt.operation_id == context.operation_id)
        .map(|attempt| attempt.state)
}

fn is_active(state: Option<AttemptState>) -> bool {
    matches!(
        state,
        Some(
            AttemptState::NeedsApproval
                | AttemptState::InProgress
                | AttemptState::AwaitingVerification
        )
    )
}

fn transition(context: &AttemptContext, action: RecoveryAction) -> SetupTaskTransitionV1 {
    match action {
        RecoveryAction::Cancel => SetupTaskTransitionV1::CancelAttempt {
            task_id: context.task_id.clone(),
            operation_id: context.operation_id.clone(),
            at_unix_ms: now_ms(),
        },
        RecoveryAction::Fail(failure) => SetupTaskTransitionV1::FailAttempt {
            task_id: context.task_id.clone(),
            operation_id: context.operation_id.clone(),
            failure,
            at_unix_ms: now_ms(),
        },
        RecoveryAction::ReconcileVerification => {
            unreachable!("verification recovery does not write a terminal transition")
        }
    }
}

fn now_ms() -> u64 {
    crate::persistence::harness_settings::current_unix_ms()
}
