use super::{AuthorizationBindingV1, FailureCode};

/// Typed state-store mutation request. These commands only record intent and
/// lifecycle; applying one never executes a host operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SetupTaskTransitionV1 {
    RequestAttempt {
        task_id: String,
        operation_id: String,
        version: u16,
        requested_scopes: Vec<String>,
        at_unix_ms: u64,
    },
    RecordAuthorization {
        task_id: String,
        operation_id: String,
        binding: AuthorizationBindingV1,
        at_unix_ms: u64,
    },
    AwaitVerification {
        task_id: String,
        operation_id: String,
        at_unix_ms: u64,
    },
    FailAttempt {
        task_id: String,
        operation_id: String,
        failure: FailureCode,
        at_unix_ms: u64,
    },
    CancelAttempt {
        task_id: String,
        operation_id: String,
        at_unix_ms: u64,
    },
    PauseAttempt {
        task_id: String,
        operation_id: String,
        at_unix_ms: u64,
    },
}
