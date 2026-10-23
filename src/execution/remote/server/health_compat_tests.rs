use super::*;
use std::io::Cursor;

/// Production boundary: stdio request validation accepts only the legacy
/// health bootstrap while preserving protocol 2 for the response and later
/// RPCs. Protocol-only tests cannot prove the dispatcher accepts it; this uses
/// a fake backend with no SSH, Docker, or network dependency (low setup cost).
#[test]
fn stdio_worker_accepts_legacy_health_only_to_negotiate_protocol_two() {
    let request = EnvelopeV1 {
        version: protocol::PROTOCOL_VERSION_V1,
        request_id: "00000000-0000-4000-8000-000000000055".into(),
        execution_id: protocol::SYSTEM_IMAGE_SESSION_ID_V1.into(),
        sequence: 1,
        kind: EnvelopeKindV1::Health,
        payload: serde_json::json!({
            "wire_version":protocol::PROTOCOL_VERSION_V2,
            "system_image_key":"1111111111111111111111111111111111111111111111111111111111111111"
        }),
    };
    let mut input = Vec::new();
    protocol::write_frame(&mut input, &request).unwrap();
    let root = tempfile::tempdir().unwrap();
    let output = serve_with_journal(
        Arc::new(FakeBackend(Arc::new(AtomicUsize::new(0)))),
        input,
        root.path(),
    );

    let response = protocol::read_frame(&mut Cursor::new(output))
        .unwrap()
        .unwrap();
    assert_eq!(response.version, protocol::PROTOCOL_VERSION_V2);
    assert_eq!(response.kind, EnvelopeKindV1::Acknowledge);
    assert_eq!(response.request_id, request.request_id);
    assert_eq!(
        response.payload["wire_version"],
        protocol::PROTOCOL_VERSION_V2
    );
    assert!(root.path().read_dir().unwrap().next().is_none());
}
