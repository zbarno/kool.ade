use super::*;

#[test]
fn peer_pid_ioctl_matches_the_connected_windows_af_unix_client() {
    use windows_sys::Win32::Networking::WinSock::{SOCKADDR, connect, socket};

    ensure_winsock().expect("initialize Winsock");
    static NEXT_PATH: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "koolade-peer-pid-{}-{}.sock",
        std::process::id(),
        NEXT_PATH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let listener = WindowsAfUnixListener::bind(&root).expect("bind AF_UNIX listener");
    // SAFETY: Winsock is initialized and the requested family/type are supported.
    let client = unsafe { socket(AF_UNIX.into(), SOCK_STREAM, 0) };
    assert_ne!(client, INVALID_SOCKET);
    let client = SocketHandle(client);
    let path = root.to_str().expect("UTF-8 short test path");
    let mut address = SOCKADDR_UN::default();
    address.sun_family = AF_UNIX;
    for (slot, byte) in address.sun_path.iter_mut().zip(path.as_bytes()) {
        *slot = *byte as i8;
    }
    // SAFETY: address contains a valid short absolute AF_UNIX path.
    let connected = unsafe {
        connect(
            client.0,
            (&address as *const SOCKADDR_UN).cast::<SOCKADDR>(),
            std::mem::size_of::<SOCKADDR_UN>() as i32,
        )
    };
    assert_eq!(connected, 0, "connect AF_UNIX fixture");
    let deadline = Instant::now() + Duration::from_secs(2);
    let accepted = loop {
        // SAFETY: listener socket is valid and address outputs are unused.
        let accepted = unsafe {
            accept(
                listener.socket.0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if accepted != INVALID_SOCKET {
            break SocketHandle(accepted);
        }
        assert_eq!(last_winsock_error().raw_os_error(), Some(WSAEWOULDBLOCK));
        assert!(Instant::now() < deadline, "AF_UNIX accept deadline");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(
        query_peer_pid(accepted.0).expect("query AF_UNIX peer PID"),
        std::process::id()
    );
    assert!(verify_peer_pid(accepted.0, std::process::id()).is_ok());
    assert_eq!(
        verify_peer_pid(accepted.0, std::process::id().wrapping_add(1))
            .expect_err("reject a different process")
            .kind(),
        io::ErrorKind::PermissionDenied
    );
}
