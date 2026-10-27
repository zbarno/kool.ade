use super::{
    ModelCatalogProbe, configured_loopback_url, explicit_loopback_catalog_url, parse_http_body,
    probe_model_catalog,
};
use std::{io::Write, net::TcpListener, thread};

#[test]
fn accepts_only_direct_loopback_http_v1_catalog_endpoints() {
    for endpoint in [
        "http://127.0.0.1:1234/v1",
        "http://[::1]:1234/v1/",
        "http://localhost:1234/v1",
    ] {
        assert!(
            explicit_loopback_catalog_url(endpoint).is_some(),
            "{endpoint}"
        );
    }
    assert_eq!(
        configured_loopback_url("http://localhost:11434/v1")
            .unwrap()
            .host_str(),
        Some("127.0.0.1")
    );
    for endpoint in [
        "https://127.0.0.1:1234/v1",
        "http://synthetic-secret@127.0.0.1:1234/v1",
        "http://127.0.0.1:1234/v1?token=synthetic-secret",
        "http://127.0.0.1:1234/v1#fragment",
        "http://127.0.0.1:1234/api",
        "http://127.0.0.1:1234/v1@remote.example.test",
        "http://192.0.2.1:1234/v1",
        "http://localhost.example.test:1234/v1",
    ] {
        assert!(
            explicit_loopback_catalog_url(endpoint).is_none(),
            "{endpoint}"
        );
    }
}

/// Production boundary: the catalog adapter sends a credential-free request to a loopback
/// endpoint and parses a live HTTP response. Parser-only tests cannot prove the socket target,
/// request headers, and response framing together. External dependency: one ephemeral loopback
/// TCP listener; setup cost: one local request and server thread.
#[test]
fn reads_models_from_bounded_non_redirecting_loopback_catalog() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(std::time::Instant::now() < deadline);
                    thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => panic!("local catalog accept failed: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .unwrap();
        let mut request = [0_u8; 1024];
        let bytes = std::io::Read::read(&mut stream, &mut request).unwrap();
        let request = std::str::from_utf8(&request[..bytes]).unwrap();
        assert!(request.starts_with("GET /v1/models HTTP/1.0\r\n"));
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
        let body = r#"{"data":[{"id":"llama3.2"},{"id":"other-model"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    let endpoint = format!("http://{address}/v1");
    let ModelCatalogProbe::Available(models) =
        probe_model_catalog(&explicit_loopback_catalog_url(&endpoint).unwrap())
    else {
        panic!("loopback model catalog should be available");
    };
    server.join().unwrap();
    assert!(models.contains("llama3.2"));
    assert!(models.contains("other-model"));
}

/// Production boundary: an unauthenticated local request maps an HTTP 401 to typed auth evidence.
/// Parser-only tests cannot prove the adapter returns that evidence without reading a response
/// body that could contain sensitive server text. External dependency: one ephemeral loopback
/// TCP listener; setup cost: one local request and server thread.
#[test]
fn reports_an_explicit_local_auth_challenge_without_reading_its_body() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(std::time::Instant::now() < deadline);
                    thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => panic!("local catalog accept failed: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .unwrap();
        let mut request = [0_u8; 512];
        let _ = std::io::Read::read(&mut stream, &mut request).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
    });
    let endpoint = format!("http://{address}/v1");
    assert!(matches!(
        probe_model_catalog(&explicit_loopback_catalog_url(&endpoint).unwrap()),
        ModelCatalogProbe::AuthenticationRequired
    ));
    server.join().unwrap();
}

#[test]
fn rejects_redirects_and_incomplete_catalog_responses() {
    for response in [
        b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1/next\r\nContent-Length: 0\r\n\r\n"
            .as_slice(),
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nshort",
    ] {
        assert!(parse_http_body(response).is_none());
    }
}
