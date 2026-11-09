//! Production boundary: the VM backend's local display proxy negotiates only
//! bounded RFB frames and typed input over its private Unix socket. A unit
//! protocol test alone cannot prove QEMU socket ownership; these tests use a
//! real Unix listener with synthetic RFB peers (no QEMU/KVM dependency).

use std::{
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
};

use super::super::super::types::ComputerDisplayInputV1;

fn listener() -> (tempfile::TempDir, UnixListener, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.path().join("display.sock");
    let listener = UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    (dir, listener, path)
}

fn server_init(stream: &mut std::os::unix::net::UnixStream) {
    stream.write_all(b"RFB 003.008\n").unwrap();
    let mut version = [0; 12];
    stream.read_exact(&mut version).unwrap();
    assert_eq!(&version, b"RFB 003.008\n");
    stream.write_all(&[1, 1]).unwrap();
    let mut selected = [0; 1];
    stream.read_exact(&mut selected).unwrap();
    assert_eq!(selected, [1]);
    stream.write_all(&0_u32.to_be_bytes()).unwrap();
    let mut shared = [0; 1];
    stream.read_exact(&mut shared).unwrap();
    assert_eq!(shared, [1]);
    stream.write_all(&2_u16.to_be_bytes()).unwrap();
    stream.write_all(&1_u16.to_be_bytes()).unwrap();
    stream.write_all(&[0; 16]).unwrap();
    stream.write_all(&0_u32.to_be_bytes()).unwrap();
}

#[test]
fn capture_reads_a_bounded_raw_frame_and_encodes_png() {
    let (_dir, listener, path) = listener();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        server_init(&mut stream);
        let mut requests = [0; 38];
        stream.read_exact(&mut requests).unwrap();
        assert_eq!(requests[0], 0); // fixed pixel format
        assert_eq!(requests[20], 2); // raw encoding only
        assert_eq!(requests[28], 3); // full framebuffer request
        assert_eq!(&requests[34..38], &[0, 2, 0, 1]);
        stream.write_all(&[0, 0, 0, 1]).unwrap();
        stream.write_all(&[0, 0, 0, 0, 0, 2, 0, 1]).unwrap();
        stream.write_all(&0_i32.to_be_bytes()).unwrap();
        stream.write_all(&[0, 0, 255, 0, 0, 255, 0, 0]).unwrap();
    });
    let frame = super::capture(&path).unwrap();
    assert_eq!((frame.width, frame.height), (2, 1));
    let decoded = image::load_from_memory(&frame.png).unwrap().to_rgba8();
    assert_eq!(decoded.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(decoded.get_pixel(1, 0).0, [0, 255, 0, 255]);
    server.join().unwrap();
}

#[test]
fn pointer_input_is_typed_and_sent_only_after_local_socket_handshake() {
    let (_dir, listener, path) = listener();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        server_init(&mut stream);
        let mut input = [0; 6];
        stream.read_exact(&mut input).unwrap();
        assert_eq!(&input, &[5, 1, 0, 1, 0, 0]);
    });
    super::send_input(
        &path,
        &ComputerDisplayInputV1::Pointer {
            x: 1,
            y: 0,
            buttons: 1,
        },
    )
    .unwrap();
    server.join().unwrap();
}

#[test]
fn rejects_nonlocal_or_unbounded_display_inputs() {
    assert!(super::valid_keysym(0x61));
    assert!(super::valid_keysym(0xff0d));
    assert!(!super::valid_keysym(0));
    assert!(super::valid_dimensions(1_920, 1_080));
    assert!(!super::valid_dimensions(1_921, 1));
    assert!(!super::valid_dimensions(0, 1));
}
