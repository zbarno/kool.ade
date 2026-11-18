use super::path::{
    canonical_session_id, contains_ambiguous_windows_component, qemu_args, qemu_option_path,
    validate_qemu_path,
};
use super::*;

#[test]
fn qemu_option_paths_reject_delimiter_injection_and_relative_paths() {
    assert_eq!(
        validate_qemu_path(Path::new("relative/qemu.exe")),
        Err(WindowsLaunchBlocker::UnsafePath)
    );
    assert_eq!(
        qemu_option_path(Path::new("C:/VMs/overlay,network=on.qcow2")),
        Err(WindowsLaunchBlocker::UnsafePath)
    );
    assert_eq!(
        validate_qemu_path(Path::new("C:/VMs/../overlay.qcow2")),
        Err(WindowsLaunchBlocker::UnsafePath)
    );
    assert_eq!(
        validate_qemu_path(Path::new("C:/VMs/NUL.qcow2")),
        Err(WindowsLaunchBlocker::UnsafePath)
    );
}

#[test]
fn qemu_paths_reject_windows_ads_and_normalized_component_aliases() {
    for path in [
        "C:/VMs/qemu.exe:payload",
        "C:/VMs/directory./qemu.exe",
        "C:/VMs/directory /qemu.exe",
    ] {
        assert!(
            contains_ambiguous_windows_component(Path::new(path)),
            "{path}"
        );
    }
    assert!(!contains_ambiguous_windows_component(Path::new(
        "C:/Program Files/Koolade/qemu-system-x86_64.exe"
    )));
    assert!(!contains_ambiguous_windows_component(Path::new(
        "\\\\server\\share\\qemu.exe"
    )));
}

#[test]
fn session_ids_must_be_canonical_before_becoming_transport_paths() {
    assert!(canonical_session_id("123e4567-e89b-12d3-a456-426614174000"));
    assert!(!canonical_session_id("../control"));
    assert!(!canonical_session_id(
        "123E4567-E89B-12D3-A456-426614174000"
    ));
}

#[test]
fn qemu_args_use_whpx_bounded_resources_and_no_network_devices() {
    let args = qemu_args(
        Path::new("C:/private/overlay.qcow2"),
        Path::new("C:/private/session/boot.json"),
        Path::new(
            "C:/private/transport/123e4567-e89b-12d3-a456-426614174000/guest-control-v1.sock",
        ),
        2,
        1_073_741_824,
    )
    .unwrap();
    let args = args
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>();
    assert!(args.windows(2).any(|w| w == ["-accel", "whpx"]));
    assert!(args.windows(2).any(|w| w == ["-m", "1073741824"]));
    assert!(args.windows(2).any(|w| w == ["-nic", "none"]));
    for forbidden in ["-net", "-netdev", "-virtfs", "-fsdev", "-smb"] {
        assert!(!args.iter().any(|arg| arg == forbidden), "{forbidden}");
    }
    assert!(!args.iter().any(|arg| arg.contains("hostfwd=")));
    assert!(!args.iter().any(|arg| arg.starts_with("tcp:")));
    assert!(!args.iter().any(|arg| arg.contains("host_path")));
    assert_eq!(
        args.iter()
            .filter(|arg| arg.as_ref() == "-blockdev")
            .count(),
        1
    );
    assert!(
        args.iter()
            .any(|arg| arg.contains("C:/private/overlay.qcow2"))
    );
    assert!(args.iter().any(|arg| {
        arg == "socket,id=guestcontrol,path=C:/private/transport/123e4567-e89b-12d3-a456-426614174000/guest-control-v1.sock,server=off"
    }));
}

#[test]
fn launch_plan_rejects_non_disposable_networked_and_mismatched_disk_inputs() {
    let base = WindowsWpxLaunchInput {
        qemu_executable: Path::new("/app/qemu-system-x86_64.exe"),
        overlay: Path::new("/app/state/overlays/session.qcow2"),
        overlay_root: Path::new("/app/state/overlays"),
        overlay_virtual_size_bytes: 2 * 1024 * 1024 * 1024,
        boot_config: Path::new("/app/state/sessions/session/boot.json"),
        session_directory: Path::new("/app/state/sessions/session"),
        guest_control_socket: Path::new(
            "/app/state/transport/123e4567-e89b-12d3-a456-426614174000/guest-control-v1.sock",
        ),
        session_id: "123e4567-e89b-12d3-a456-426614174000",
        cpu_millis: 2_000,
        memory_bytes: 1024 * MIB,
        disk_bytes: 2 * 1024 * 1024 * 1024,
        disposable_session: true,
        network_disabled: true,
    };
    assert!(WindowsWpxLaunchPlan::build(base, 8).is_ok());
    assert_eq!(
        WindowsWpxLaunchPlan::build(
            WindowsWpxLaunchInput {
                disposable_session: false,
                ..base
            },
            8,
        )
        .unwrap_err(),
        WindowsLaunchBlocker::UnsupportedSessionClass
    );
    assert_eq!(
        WindowsWpxLaunchPlan::build(
            WindowsWpxLaunchInput {
                network_disabled: false,
                ..base
            },
            8,
        )
        .unwrap_err(),
        WindowsLaunchBlocker::NetworkNotDisabled
    );
    assert_eq!(
        WindowsWpxLaunchPlan::build(
            WindowsWpxLaunchInput {
                overlay_virtual_size_bytes: 123,
                ..base
            },
            8,
        )
        .unwrap_err(),
        WindowsLaunchBlocker::InvalidOverlay
    );
}

#[test]
fn launch_plan_maps_guest_and_job_limits_from_the_bounded_policy() {
    let plan = WindowsWpxLaunchPlan::build(
        WindowsWpxLaunchInput {
            qemu_executable: Path::new("/app/qemu-system-x86_64.exe"),
            overlay: Path::new("/app/state/overlays/session.qcow2"),
            overlay_root: Path::new("/app/state/overlays"),
            overlay_virtual_size_bytes: 2 * 1024 * 1024 * 1024,
            boot_config: Path::new("/app/state/sessions/session/boot.json"),
            session_directory: Path::new("/app/state/sessions/session"),
            guest_control_socket: Path::new(
                "/app/state/transport/123e4567-e89b-12d3-a456-426614174000/guest-control-v1.sock",
            ),
            session_id: "123e4567-e89b-12d3-a456-426614174000",
            cpu_millis: 2_000,
            memory_bytes: 1024 * MIB,
            disk_bytes: 2 * 1024 * 1024 * 1024,
            disposable_session: true,
            network_disabled: true,
        },
        8,
    )
    .unwrap();
    assert_eq!(
        plan.resource_summary(),
        (2, 1024 * MIB, 2 * 1024 * 1024 * 1024)
    );
    assert_eq!(
        plan.control_socket_path(),
        Path::new(
            "/app/state/transport/123e4567-e89b-12d3-a456-426614174000/guest-control-v1.sock"
        )
    );
    assert!(plan.arguments().iter().any(|arg| arg == "-accel"));
    assert_eq!(
        plan.job_resource_limits(),
        (2 * 1024 * 1024 * 1024, 2_500, 1)
    );
}
