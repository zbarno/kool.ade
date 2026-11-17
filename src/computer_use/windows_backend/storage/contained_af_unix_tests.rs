//! Native process-to-listener boundary fixture. This validates Windows
//! containment and AF_UNIX peer identity; it does not test a QEMU build.

use super::super::WindowsVmStorage;
use std::{
    process::Command,
    time::{Duration, Instant},
};
use windows_sys::Win32::Networking::WinSock::{
    AF_UNIX, INVALID_SOCKET, SOCK_STREAM, SOCKADDR, SOCKADDR_UN, SOCKET, WSADATA, WSAStartup,
    closesocket, connect, socket,
};

const CHILD_SOCKET_ENV: &str = "KOOLADE_TEST_WINDOWS_AF_UNIX_CHILD_SOCKET";
const QEMU_BINARY_ENV: &str = "KOOLADE_TEST_WINDOWS_QEMU";
const CHILD_TEST_FILTER: &str = "computer_use::windows_backend::storage::tests::contained_af_unix_tests::contained_guest_client_connects_under_job";

/// Boundary: a current-test-executable child is created suspended, assigned
/// to its resource-limited Job Object, resumed, and accepted by the app-owned
/// storage listener only when Winsock reports that child's PID. Unit tests
/// cannot prove kernel Job Object assignment or AF_UNIX peer-PID behavior.
/// Dependency: native Windows process, Job Object, ACL, and Winsock APIs; no
/// QEMU is started and setup is bounded to under 2 seconds.
#[test]
fn contained_guest_client_connects_under_job() {
    if let Some(path) = std::env::var_os(CHILD_SOCKET_ENV) {
        child_connect_and_wait(&path);
    }

    let temporary = super::af_unix_tests::ShortStorageRoot::new();
    let storage = WindowsVmStorage::open_at(temporary.path()).expect("private test storage");
    let session_id = "123e4567-e89b-12d3-a456-426614174000";
    let socket_path = storage
        .guest_control_socket_path(session_id)
        .expect("app-owned socket path");
    assert!(socket_path.to_string_lossy().len() < 108);
    let listener = storage
        .bind_guest_control_listener(session_id)
        .expect("bind app-owned guest listener");

    let limits = super::super::super::WindowsVmJobLimits::new(1024 * 1024 * 1024, 1_000, 1)
        .expect("bounded child limits");
    let mut command = Command::new(std::env::current_exe().expect("current test executable"));
    command.args(["--exact", CHILD_TEST_FILTER, "--nocapture"]);
    let mut child =
        super::super::super::process::WindowsVmProcessJob::spawn_contained_with_test_socket_path(
            command,
            limits,
            &socket_path,
        )
        .expect("create, assign, verify, then resume test client");

    let peer_pid = listener
        .accept_child_peer_pid_for_test(&mut child, Duration::from_secs(5))
        .expect("accept only the active contained child");
    assert_eq!(
        peer_pid,
        child.id(),
        "accepted AF_UNIX peer must be the child"
    );

    let started = Instant::now();
    let status = child
        .terminate(Duration::from_secs(5))
        .expect("bounded Job Object termination");
    assert!(started.elapsed() <= Duration::from_secs(6));
    assert!(
        status.code().is_some(),
        "terminated child has a final status"
    );
    assert!(child.try_wait().expect("reap child").is_some());

    drop(listener);
    drop(storage);
    let temporary_path = temporary.path().to_path_buf();
    drop(temporary);
    assert!(
        !temporary_path.exists(),
        "temporary app-owned storage is removed"
    );
}

/// Integration boundary: run the trusted MSYS2 QEMU package as a contained
/// client of the app-owned Windows AF_UNIX listener. This proves the actual
/// QEMU socket chardev mode and Windows peer-PID API agree; it does not certify
/// WHPX availability, guest boot, display forwarding, or the production image.
#[test]
fn qemu_af_unix_client_connects_from_inside_a_job() {
    let qemu = std::path::PathBuf::from(
        std::env::var_os(QEMU_BINARY_ENV)
            .expect("native Windows CI must provide QEMU from the signed MSYS2 package"),
    );
    assert!(qemu.is_absolute(), "QEMU binary path must be absolute");
    assert!(qemu.is_file(), "QEMU binary must exist");
    let qemu_data = qemu
        .parent()
        .and_then(std::path::Path::parent)
        .map(|prefix| prefix.join("share").join("qemu"))
        .expect("MSYS2 QEMU binary has a package prefix");
    assert!(qemu_data.is_dir(), "QEMU package data directory must exist");

    let temporary = super::af_unix_tests::ShortStorageRoot::new();
    let storage = WindowsVmStorage::open_at(temporary.path()).expect("private test storage");
    let session_id = "123e4567-e89b-12d3-a456-426614174001";
    let socket_path = storage
        .guest_control_socket_path(session_id)
        .expect("app-owned guest listener path");
    assert!(socket_path.to_string_lossy().len() < 108);
    assert!(
        !socket_path.to_string_lossy().contains(','),
        "QEMU key-value socket path must not contain option delimiters"
    );
    let listener = storage
        .bind_guest_control_listener(session_id)
        .expect("bind app-owned guest listener");

    let limits = super::super::super::WindowsVmJobLimits::new(2 * 1024 * 1024 * 1024, 1_000, 4)
        .expect("bounded QEMU test limits");
    let mut command = Command::new(&qemu);
    command.args([
        "-no-user-config",
        "-nodefaults",
        "-L",
        qemu_data.to_str().expect("QEMU data path is UTF-8"),
        "-machine",
        "q35",
        "-accel",
        "tcg",
        "-smp",
        "1",
        "-m",
        "512",
        "-display",
        "none",
        "-vnc",
        "none",
        "-nic",
        "none",
        "-monitor",
        "none",
        "-serial",
        "none",
        "-parallel",
        "none",
    ]);
    command.arg("-chardev").arg(format!(
        "socket,id=guestcontrol,path={},server=off",
        socket_path.display()
    ));
    command.args([
        "-device",
        "virtio-serial-pci",
        "-device",
        "virtserialport,chardev=guestcontrol,name=org.koolade.computer-control.v1",
        "-S",
    ]);
    let mut qemu =
        super::super::super::process::WindowsVmProcessJob::spawn_contained_with_test_socket_path(
            command,
            limits,
            &socket_path,
        )
        .expect("start QEMU suspended, contain it, then resume");

    let peer_pid = listener
        .accept_child_peer_pid_for_test(&mut qemu, Duration::from_secs(20))
        .expect("QEMU socket chardev connects as the contained QEMU child");
    assert_eq!(peer_pid, qemu.id(), "QEMU peer identity is the job child");

    let started = Instant::now();
    qemu.terminate(Duration::from_secs(10))
        .expect("terminate QEMU job within a bound");
    assert!(started.elapsed() <= Duration::from_secs(11));
    assert!(qemu.try_wait().expect("reap contained QEMU").is_some());

    drop(listener);
    drop(storage);
    let temporary_path = temporary.path().to_path_buf();
    drop(temporary);
    assert!(
        !temporary_path.exists(),
        "temporary app-owned QEMU test storage is removed"
    );
}

fn child_connect_and_wait(path: &std::ffi::OsStr) -> ! {
    let mut data = WSADATA::default();
    assert_eq!(unsafe { WSAStartup(0x0202, &mut data) }, 0, "WSAStartup");
    let client = unsafe { socket(AF_UNIX.into(), SOCK_STREAM, 0) };
    assert_ne!(client, INVALID_SOCKET, "create AF_UNIX client socket");
    let client = ClientSocket(client);

    let path = std::path::Path::new(path)
        .to_str()
        .expect("AF_UNIX path is UTF-8");
    assert!(path.len() < 108, "AF_UNIX path is bounded");
    let mut address = SOCKADDR_UN {
        sun_family: AF_UNIX,
        ..Default::default()
    };
    for (slot, byte) in address.sun_path.iter_mut().zip(path.as_bytes()) {
        *slot = *byte as i8;
    }
    assert_eq!(
        unsafe {
            connect(
                client.0,
                (&address as *const SOCKADDR_UN).cast::<SOCKADDR>(),
                std::mem::size_of::<SOCKADDR_UN>() as i32,
            )
        },
        0,
        "connect to app-owned AF_UNIX listener"
    );
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

struct ClientSocket(SOCKET);

impl Drop for ClientSocket {
    fn drop(&mut self) {
        unsafe { closesocket(self.0) };
        unsafe { windows_sys::Win32::Networking::WinSock::WSACleanup() };
    }
}
