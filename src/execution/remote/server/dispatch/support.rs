use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::execution::errors::ExecutionError;
use crate::execution::remote::protocol::EnvelopeV1;

const CONTROL_EXECUTION_ID: &str = "00000000-0000-4000-8000-000000000000";

pub(super) fn decode<T: DeserializeOwned>(value: &Value) -> Result<T, ExecutionError> {
    serde_json::from_value(value.clone())
        .map_err(|_| super::super::incompatible("remote worker request payload is invalid"))
}

pub(super) fn json_value(value: impl serde::Serialize) -> Result<Value, ExecutionError> {
    serde_json::to_value(value)
        .map_err(|_| super::super::incompatible("worker response is invalid"))
}

pub(super) fn require_control_id(envelope: &EnvelopeV1) -> Result<(), ExecutionError> {
    require_execution_id(envelope, CONTROL_EXECUTION_ID)
}

pub(super) fn require_execution_id(
    envelope: &EnvelopeV1,
    expected: &str,
) -> Result<(), ExecutionError> {
    if envelope.execution_id == expected {
        Ok(())
    } else {
        Err(super::super::incompatible(
            "remote worker execution identity does not match payload",
        ))
    }
}
