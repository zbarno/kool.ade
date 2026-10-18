use super::{preserve_missing_runtime_wait, should_reconcile_before_command};
use crate::persistence::execution_records::ExecutionRecordStateV1 as State;

#[test]
fn restart_preserves_distinct_durable_wait_states_before_command_start() {
    for state in [
        State::WaitingForDependencyApproval,
        State::WaitingForRuntime,
    ] {
        assert!(
            !should_reconcile_before_command(false, state),
            "{state:?} must be restored as a wait, not discarded as an incomplete start"
        );
        assert!(
            !should_reconcile_before_command(true, state),
            "the started-command recovery branch handles these records first"
        );
    }
    assert!(should_reconcile_before_command(false, State::Preparing));
}

#[test]
fn missing_container_404_preserves_a_pre_command_runtime_wait_for_retry() {
    assert!(preserve_missing_runtime_wait(
        State::WaitingForRuntime,
        false,
        false
    ));
    assert!(!preserve_missing_runtime_wait(
        State::WaitingForRuntime,
        true,
        false
    ));
    assert!(!preserve_missing_runtime_wait(State::Running, false, false));
    assert!(!preserve_missing_runtime_wait(
        State::WaitingForDependencyApproval,
        false,
        false
    ));
}
