use super::*;
use std::net::TcpListener;

#[test]
fn non_generating_probe_sends_sensitive_auth_and_promotes_only_active_selectable_models() {
    let (endpoint, request) = mock_response(
        "200 OK",
        r#"{"data":[{"id":"claude-live","lifecycle":"active"},{"id":"claude-retired","lifecycle":"retired"}],"has_more":false}"#,
        "",
    );
    let result = verify_api_key_at(
        &endpoint,
        "synthetic-api-key",
        &[
            "anthropic/claude-live".into(),
            "anthropic/claude-retired".into(),
            "openai/gpt-test".into(),
        ],
    );
    assert!(
        matches!(result, Verification::Verified(models) if models == ["anthropic/claude-live"])
    );
    let request = request.recv().unwrap();
    assert!(request.starts_with("GET /v1/models?limit=1000 HTTP/1.1\r\n"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer synthetic-api-key\r\n")
    );
    assert!(
        request
            .to_ascii_lowercase()
            .contains("anthropic-version: 2026-10-21\r\n")
    );
}

#[test]
fn auth_errors_are_unavailable_while_server_and_schema_errors_remain_unknown() {
    for (status, body, expected) in [
        ("401 Unauthorized", "{}", "rejected"),
        ("403 Forbidden", "{}", "rejected"),
        ("503 Service Unavailable", "{}", "unknown"),
        (
            "200 OK",
            r#"{"data":[{"id":"claude-test","lifecycle":"active"}],"has_more":true}"#,
            "unknown",
        ),
    ] {
        let (endpoint, _request) = mock_response(status, body, "");
        let result = verify_api_key_at(&endpoint, "synthetic-api-key", &[]);
        assert_eq!(
            matches!(result, Verification::Rejected),
            expected == "rejected",
            "unexpected result for HTTP {status}"
        );
    }
}

#[test]
fn redirect_is_not_followed_and_oversized_model_catalog_is_rejected() {
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    target.set_nonblocking(true).unwrap();
    let redirect = format!("http://{}/", target.local_addr().unwrap());
    let (endpoint, _request) = mock_response("302 Found", "", &format!("Location: {redirect}\r\n"));
    assert!(matches!(
        verify_api_key_at(&endpoint, "synthetic-api-key", &[]),
        Verification::Unknown
    ));
    assert!(
        matches!(target.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );

    let (endpoint, _request) = mock_response(
        "200 OK",
        "",
        &format!("Content-Length: {}\r\n", RESPONSE_LIMIT + 1),
    );
    assert!(matches!(
        verify_api_key_at(&endpoint, "synthetic-api-key", &[]),
        Verification::Unknown
    ));
}

/// This test crosses reqwest's real request, header, status, redirect, and bounded-body boundary.
/// Parser tests alone cannot establish those properties. It uses only a loopback TCP mock server,
/// with no TLS credentials, provider account, external network, Docker, or SSH; setup is small.
fn mock_response(
    status: &str,
    body: &str,
    extra_headers: &str,
) -> (String, std::sync::mpsc::Receiver<String>) {
    use std::io::{Read, Write};
    use std::sync::mpsc;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!(
        "http://{}/v1/models?limit=1000",
        listener.local_addr().unwrap()
    );
    let (request_tx, request_rx) = mpsc::channel();
    let status = status.to_owned();
    let body = body.to_owned();
    let extra_headers = extra_headers.to_owned();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let read = stream.read(&mut chunk).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&chunk[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let _ = request_tx.send(String::from_utf8_lossy(&request).into_owned());
        let content_length = if extra_headers
            .to_ascii_lowercase()
            .contains("content-length:")
        {
            String::new()
        } else {
            format!("Content-Length: {}\r\n", body.len())
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{content_length}Connection: close\r\n{extra_headers}\r\n{body}"
        );
        let _ = stream.write_all(response.as_bytes());
    });
    (endpoint, request_rx)
}
