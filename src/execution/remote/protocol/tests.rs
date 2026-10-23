use super::*;
use crate::execution::remote::private_files::PrivateFileChunkV1;
use std::io::Cursor;

fn envelope() -> EnvelopeV1 {
    EnvelopeV1 {
        version: PROTOCOL_VERSION_V2,
        request_id: "00000000-0000-4000-8000-000000000001".into(),
        execution_id: "00000000-0000-4000-8000-000000000002".into(),
        sequence: 1,
        kind: EnvelopeKindV1::Probe,
        payload: serde_json::json!({"target": "local"}),
    }
}

#[test]
fn round_trip_uses_bounded_length_prefixed_json() {
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &envelope()).unwrap();
    let mut input = Cursor::new(bytes);
    assert_eq!(read_frame(&mut input).unwrap(), Some(envelope()));
    assert_eq!(read_frame(&mut input).unwrap(), None);
}

#[test]
fn legacy_protocol_is_limited_to_the_single_version_two_health_bootstrap() {
    let request = EnvelopeV1 {
        version: PROTOCOL_VERSION_V1,
        request_id: "00000000-0000-4000-8000-000000000003".into(),
        execution_id: SYSTEM_IMAGE_SESSION_ID_V1.into(),
        sequence: 1,
        kind: EnvelopeKindV1::Health,
        payload: serde_json::json!({
            "wire_version": PROTOCOL_VERSION_V2,
            "system_image_key":"1111111111111111111111111111111111111111111111111111111111111111"
        }),
    };
    let mut wire = Vec::new();
    write_frame(&mut wire, &request).unwrap();
    assert_eq!(
        read_frame(&mut Cursor::new(wire)).unwrap(),
        Some(request.clone())
    );

    let mut validator = SessionValidator::default();
    validator.accept_compat_health_request(&request).unwrap();
    assert!(validator.accept_compat_health_request(&request).is_err());

    let mut bootstrap_without_key = request.clone();
    bootstrap_without_key.payload = serde_json::json!({"wire_version": PROTOCOL_VERSION_V2});
    assert!(
        SessionValidator::default()
            .accept_compat_health_request(&bootstrap_without_key)
            .is_err()
    );

    let response = EnvelopeV1 {
        version: PROTOCOL_VERSION_V1,
        request_id: request.request_id.clone(),
        execution_id: request.execution_id.clone(),
        sequence: request.sequence,
        kind: EnvelopeKindV1::Error,
        payload: serde_json::json!({"kind":"worker_incompatible","message":"unsupported","retryable":false}),
    };
    let mut validator = SessionValidator::default();
    validator.accept_compat_health_response(&response).unwrap();

    let mut non_health = request;
    non_health.kind = EnvelopeKindV1::Exec;
    assert!(write_frame(Vec::new(), &non_health).is_err());
    assert!(
        SessionValidator::default()
            .accept_compat_health_request(&non_health)
            .is_err()
    );
}

#[test]
fn oversized_and_truncated_frames_are_rejected_before_decode() {
    let mut oversized = Cursor::new(((MAX_FRAME_BYTES as u32) + 1).to_be_bytes());
    assert_eq!(
        read_frame(&mut oversized).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );

    let mut truncated = Cursor::new([0, 0, 0, 8, b'{', b'}']);
    assert_eq!(
        read_frame(&mut truncated).unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
}

#[test]
fn rejects_unknown_fields_and_incompatible_versions() {
    let mut bytes = Vec::new();
    let mut bad = envelope();
    bad.version = PROTOCOL_VERSION_V2 - 1;
    assert_eq!(
        write_frame(&mut bytes, &bad).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );

    let json = br#"{"version":2,"request_id":"00000000-0000-4000-8000-000000000001","execution_id":"00000000-0000-4000-8000-000000000002","sequence":1,"kind":"probe","payload":{},"extra":true}"#;
    let mut framed = (json.len() as u32).to_be_bytes().to_vec();
    framed.extend_from_slice(json);
    assert_eq!(
        read_frame(&mut Cursor::new(framed)).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn session_rejects_duplicate_requests_and_sequence_gaps() {
    let mut validator = SessionValidator::default();
    let first = envelope();
    validator.accept(&first).unwrap();
    assert_eq!(
        validator.accept(&first).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );

    let mut next = envelope();
    next.request_id = "00000000-0000-4000-8000-000000000003".into();
    next.sequence = 3;
    assert_eq!(
        validator.accept(&next).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    next.sequence = 2;
    validator.accept(&next).unwrap();
}

#[test]
fn new_transport_session_can_resume_at_a_persisted_sequence() {
    let mut validator = SessionValidator::default();
    let mut resumed = envelope();
    resumed.sequence = 17;
    validator.accept(&resumed).unwrap();
    resumed.request_id = "00000000-0000-4000-8000-000000000003".into();
    resumed.sequence = 18;
    validator.accept(&resumed).unwrap();
    resumed.sequence = 20;
    resumed.request_id = "00000000-0000-4000-8000-000000000004".into();
    assert_eq!(
        validator.accept(&resumed).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn private_chunk_payload_is_redacted_and_cleared() {
    let mut request = envelope();
    request.kind = EnvelopeKindV1::PrivateFileChunk;
    request.payload = serde_json::json!({"chunk": {"bytes": "c2VjcmV0"}});
    let diagnostic = format!("{request:?}");
    assert!(!diagnostic.contains("c2VjcmV0"));
    assert!(!diagnostic.contains("secret"));

    request.clear_sensitive_payload();
    assert_eq!(request.payload, serde_json::Value::Null);
}

#[test]
fn health_payload_is_redacted_and_cleared() {
    let mut request = envelope();
    request.kind = EnvelopeKindV1::Health;
    request.payload = serde_json::json!({
        "wire_version": PROTOCOL_VERSION_V2,
        "system_image_key": "1111111111111111111111111111111111111111111111111111111111111111"
    });
    let diagnostic = format!("{request:?}");
    assert!(
        !diagnostic.contains("1111111111111111111111111111111111111111111111111111111111111111")
    );

    request.clear_sensitive_payload();
    assert_eq!(request.payload, serde_json::Value::Null);
}

#[test]
fn generic_frame_writer_rejects_private_chunks_before_output() {
    let mut request = envelope();
    request.kind = EnvelopeKindV1::PrivateFileChunk;
    request.payload = serde_json::json!({"bytes":"c3ludGhldGlj"});
    let mut output = Vec::new();

    let error = write_frame(&mut output, &request).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(output.is_empty());
}

#[test]
fn private_payload_raw_scan_covers_partial_envelope_parse_errors() {
    let body = br#"{"version":2,"request_id":"00000000-0000-4000-8000-000000000001","execution_id":"00000000-0000-4000-8000-000000000002","kind":"private_file_chunk","payload":{"bytes":"c2Vuc2l0aXZl"},"sequence":"invalid"}"#;
    let mut frame = (body.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(body);

    assert_eq!(
        read_frame(&mut Cursor::new(frame)).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn malformed_private_payload_is_rejected_before_value_decode() {
    let body = br#"{"version":2,"request_id":"00000000-0000-4000-8000-000000000001","execution_id":"00000000-0000-4000-8000-000000000002","sequence":1,"kind":"private_file_chunk","payload":{"bytes":"c2Vuc2l0aXZl",}}"#;
    let mut frame = (body.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(body);

    assert_eq!(
        read_frame(&mut Cursor::new(frame)).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn private_payload_inside_json_is_rejected_as_an_invalid_header() {
    let body = br#"{"version":2,"request_id":"00000000-0000-4000-8000-000000000001","execution_id":"00000000-0000-4000-8000-000000000002","sequence":1,"kind":"private_file_chunk","payload":{"bytes":"c2Vuc2l0aXZl","untrusted_number":1e9999}}"#;
    let mut frame = (body.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(body);

    let error = read_frame(&mut Cursor::new(frame)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn invalid_private_envelope_is_rejected_after_validation() {
    let request = EnvelopeV1 {
        version: PROTOCOL_VERSION_V2,
        request_id: "00000000-0000-4000-8000-000000000001".into(),
        execution_id: "00000000-0000-4000-8000-000000000002".into(),
        sequence: 0,
        kind: EnvelopeKindV1::PrivateFileChunk,
        payload: serde_json::json!({"bytes":"c2Vuc2l0aXZl"}),
    };
    let body = serde_json::to_vec(&request).unwrap();
    let mut frame = (body.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(&body);

    assert_eq!(
        read_frame(&mut Cursor::new(frame)).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn dedicated_private_chunk_frame_keeps_bytes_out_of_json_metadata() {
    let chunk = PrivateFileChunkV1::new(
        "00000000-0000-4000-8000-000000000003".into(),
        ".env.local".into(),
        0,
        b"synthetic-secret-bytes".to_vec(),
    )
    .unwrap();
    let mut request = envelope();
    request.kind = EnvelopeKindV1::PrivateFileChunk;
    request.payload = serde_json::to_value(chunk.header().unwrap()).unwrap();

    let mut wire = Vec::new();
    write_private_file_chunk_frame(&mut wire, &request, &chunk).unwrap();
    let header_len = u32::from_be_bytes(wire[..4].try_into().unwrap()) as usize;
    let header_end = 4 + header_len;
    let header = &wire[4..header_end];
    assert!(
        !header
            .windows(b"synthetic-secret-bytes".len())
            .any(|window| window == b"synthetic-secret-bytes")
    );
    assert!(
        !header
            .windows(b"c3ludGhldGljLXNlY3JldC1ieXRlcw==".len())
            .any(|window| window == b"c3ludGhldGljLXNlY3JldC1ieXRlcw==")
    );
    assert!(!format!("{request:?}").contains("synthetic-secret-bytes"));

    let decoded = read_worker_frame(&mut Cursor::new(wire)).unwrap().unwrap();
    let WorkerInboundFrameV1::PrivateFileChunk {
        envelope: received,
        chunk: received_chunk,
    } = decoded
    else {
        panic!("private chunk frame did not use its typed handler");
    };
    assert_eq!(received, request);
    assert_eq!(&*received_chunk.bytes, b"synthetic-secret-bytes");
}

#[test]
fn ordinary_reader_rejects_a_valid_typed_private_chunk_frame() {
    let chunk = PrivateFileChunkV1::new(
        "00000000-0000-4000-8000-000000000003".into(),
        ".env.local".into(),
        0,
        b"synthetic".to_vec(),
    )
    .unwrap();
    let mut request = envelope();
    request.kind = EnvelopeKindV1::PrivateFileChunk;
    request.payload = serde_json::to_value(chunk.header().unwrap()).unwrap();
    let mut wire = Vec::new();
    write_private_file_chunk_frame(&mut wire, &request, &chunk).unwrap();

    let error = read_frame(&mut Cursor::new(wire)).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}
