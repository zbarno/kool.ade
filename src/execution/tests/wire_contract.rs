use super::*;

#[test]
fn versioned_request_round_trips_without_controller_paths_or_environment_values() {
    let request = request();
    request.validate_at(1).unwrap();
    let message = ExecutionWireMessageV1 {
        wire_version: 1,
        request,
    };
    let bytes = serde_json::to_vec(&message).unwrap();
    let decoded: ExecutionWireMessageV1 = serde_json::from_slice(&bytes).unwrap();
    decoded.validate_at(1).unwrap();
    assert_eq!(decoded, message);
    assert!(ensure_wire_compatible(1).is_ok());
    assert_eq!(
        ensure_wire_compatible(2),
        Err(ExecutionErrorKind::WorkerIncompatible)
    );
    let json = String::from_utf8(bytes).unwrap();
    assert!(!json.contains("/home/"));
    assert!(!json.contains("github_token"));
}

#[test]
fn legacy_execution_requests_default_destination_sha_to_none() {
    let mut value = serde_json::to_value(request()).unwrap();
    value.as_object_mut().unwrap().remove("destination_sha");
    let decoded: crate::execution::contract::ExecutionRequestV1 =
        serde_json::from_value(value).unwrap();
    assert_eq!(decoded.destination_sha, None);
}
