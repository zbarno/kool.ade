use super::super::DockerEndpoint;
use super::PinnedUnixConnector;
use hyper::Uri;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixListener,
};
use tower_service::Service;

/// Production boundary: Bollard's pooled client and the pinned Unix transport.
/// Invariant: version and info requests reuse the same accepted socket; a
/// connector-only unit test cannot prove the HTTP pool preserves that socket.
/// Dependencies: temporary Unix socket only, no Docker daemon; low setup cost.
#[test]
fn docker_api_requests_reuse_the_original_socket_and_fail_closed_on_reconnect() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("docker.sock");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let listener = runtime.block_on(async { UnixListener::bind(&path).unwrap() });
    let server = runtime.spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        for body in [
            r#"{"Version":"28.0.0","ApiVersion":"1.53","MinAPIVersion":"1.41","Arch":"amd64"}"#,
            r#"{"ID":"approved-daemon","OSType":"linux","Architecture":"x86_64","SecurityOptions":["name=rootless"]}"#,
        ] {
            let mut line = String::new();
            loop {
                line.clear();
                if reader.read_line(&mut line).await.unwrap() == 0 {
                    panic!("Docker client closed its approved connection early");
                }
                if line == "\r\n" {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{}",
                body.len(),
                body
            );
            reader.get_mut().write_all(response.as_bytes()).await.unwrap();
            reader.get_mut().flush().await.unwrap();
        }
    });
    let endpoint = DockerEndpoint::Unix {
        path,
        desktop: false,
    };
    let docker = endpoint.connect_pinned().unwrap();

    runtime.block_on(async {
        let version = docker.version().await.unwrap();
        assert_eq!(version.version.as_deref(), Some("28.0.0"));
        let info = docker.info().await.unwrap();
        assert_eq!(info.id.as_deref(), Some("approved-daemon"));
        server.await.unwrap();
    });
}

#[test]
fn pinned_connector_refuses_a_replacement_socket_after_connection_loss() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("docker.sock");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let listener = runtime.block_on(async { UnixListener::bind(&path).unwrap() });
    let mut connector = PinnedUnixConnector {
        path: path.clone(),
        used: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let uri = Uri::from_static("http://pinned-docker/");
    let stream = runtime.block_on(connector.call(uri.clone())).unwrap();
    let (accepted, _) = runtime.block_on(listener.accept()).unwrap();
    drop(stream);
    drop(accepted);
    drop(listener);
    std::fs::remove_file(&path).unwrap();
    let _replacement = runtime.block_on(async { UnixListener::bind(&path).unwrap() });

    let error = match runtime.block_on(connector.call(uri)) {
        Ok(_) => panic!("pinned connector reopened a replacement Docker socket"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), std::io::ErrorKind::ConnectionRefused);
}

/// Production boundary: Bollard's API client after its accepted connection
/// closes. Invariant: a reconnect cannot reach a replacement socket pathname;
/// a connector unit test cannot prove the HTTP client retries fail closed.
/// Dependencies: temporary Unix sockets only, no Docker daemon; low setup cost.
#[test]
fn docker_client_fails_closed_when_reconnect_would_reach_a_replacement_socket() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("docker.sock");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let listener = runtime.block_on(async { UnixListener::bind(&path).unwrap() });
    let server = runtime.spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).await.unwrap() == 0 {
                panic!("Docker client closed its approved connection before the request");
            }
            if line == "\r\n" {
                break;
            }
        }
        let body = r#"{"Version":"28.0.0","ApiVersion":"1.53","MinAPIVersion":"1.41","Arch":"amd64"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        reader.get_mut().write_all(response.as_bytes()).await.unwrap();
        reader.get_mut().flush().await.unwrap();
    });
    let endpoint = DockerEndpoint::Unix {
        path: path.clone(),
        desktop: false,
    };
    let docker = endpoint.connect_pinned().unwrap();
    runtime.block_on(async {
        assert_eq!(
            docker.version().await.unwrap().version.as_deref(),
            Some("28.0.0")
        );
        server.await.unwrap();
    });

    std::fs::remove_file(&path).unwrap();
    let replacement = runtime.block_on(async { UnixListener::bind(&path).unwrap() });
    assert!(runtime.block_on(docker.info()).is_err());
    let replacement_accept = runtime.block_on(async {
        tokio::time::timeout(std::time::Duration::from_millis(100), replacement.accept()).await
    });
    assert!(replacement_accept.is_err());
}
