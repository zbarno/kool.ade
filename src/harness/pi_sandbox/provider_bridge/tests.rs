use super::{BOOTSTRAP, Target, relay, rewrite_header, sandbox_mounts};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::net::UnixStream,
    thread,
};

#[test]
fn provider_bridge_replaces_dummy_auth_and_rejects_other_routes() {
    let upstream = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = upstream.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = upstream.accept().unwrap();
        let request = read_header(&mut stream);
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(request.contains("Host: 127.0.0.1:"));
        assert!(request.contains("Authorization: Bearer test-secret\r\n"));
        assert!(!request.contains("koolade-proxy"));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .unwrap();
    });
    let target = Target {
        address,
        host: format!("127.0.0.1:{}", address.port()),
        prefix: "/v1/".into(),
    };
    let (mut client, server_end) = UnixStream::pair().unwrap();
    let client_thread = thread::spawn(move || {
        client
            .write_all(b"POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:8765\r\nAuthorization: Bearer koolade-proxy\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        response
    });
    relay(server_end, target.clone(), "test-secret").unwrap();
    server.join().unwrap();
    let response = String::from_utf8(client_thread.join().unwrap()).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.ends_with("ok"));

    assert!(
        rewrite_header(
            b"POST http://example.com/v1/chat/completions HTTP/1.1\r\nHost: example.com\r\n\r\n",
            &target,
            "test-secret",
        )
        .is_none()
    );
}

#[test]
fn provider_configuration_is_readonly_source_and_writable_sandbox_copy() {
    let mounts = sandbox_mounts(
        std::path::Path::new("/host/provider/socket"),
        std::path::Path::new("/host/provider/agent"),
    );
    let arguments = mounts.join(" ");
    assert!(arguments.contains("/host/provider/agent /run/koolade-provider-config/agent"));
    assert!(!arguments.contains("/host/provider/agent /tmp/koolade-home"));
    assert!(arguments.contains("--dir /tmp/koolade-home/.pi/agent"));
    assert!(BOOTSTRAP.contains("cp \"$config/settings.json\" \"$runtime/settings.json\""));
    assert!(BOOTSTRAP.contains("cp \"$config/models.json\" \"$runtime/models.json\""));
    assert!(BOOTSTRAP.contains("cp \"$config/model-relay.cjs\" \"$runtime/model-relay.cjs\""));
}

#[test]
fn provider_stream_read_timeout_matches_harness_stall_timeout() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let sender = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (receiver, _) = listener.accept().unwrap();
    let (client, _peer) = UnixStream::pair().unwrap();
    super::set_idle_read_timeouts(&sender, &client).unwrap();
    let expected = Some(crate::harness::pi_harness::configured_stall_timeout());
    assert_eq!(sender.read_timeout().unwrap(), expected);
    assert_eq!(client.read_timeout().unwrap(), expected);
    drop(receiver);
}

fn read_header(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut byte = [0; 1];
    while stream.read(&mut byte).unwrap() > 0 {
        bytes.push(byte[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(bytes).unwrap()
}
