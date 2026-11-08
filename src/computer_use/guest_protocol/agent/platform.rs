use crate::computer_use::guest_protocol::{
    AuthenticatedGuestEnvelopeV1, GuestBootConfigV1, GuestCapabilitySetV1, GuestEnvelopeContextV1,
    GuestEnvelopeV1, GuestExitReasonV1, GuestMessageV1, GuestProtocolErrorV1, GuestSenderV1,
    auth::GuestHandshakeCompleteV1, frame::write_frame,
};
use std::{
    io::Write,
    os::unix::fs::MetadataExt,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

const GUEST_APPS: &[(&str, &str)] = &[
    ("chromium", "/usr/bin/chromium"),
    ("file-manager", "/usr/bin/thunar"),
    ("text-editor", "/usr/bin/mousepad"),
    (
        "test-gui-fixture",
        "/usr/lib/koolade/computer-use-test-fixture",
    ),
];

pub(super) fn guest_capabilities(config: &GuestBootConfigV1) -> GuestCapabilitySetV1 {
    let (screen_capture, input) = super::x11::capabilities();
    guest_capabilities_with(config, trusted_executable_present, screen_capture, input)
}

fn guest_capabilities_with(
    config: &GuestBootConfigV1,
    executable_present: impl Fn(&str) -> bool,
    screen_capture: bool,
    input: bool,
) -> GuestCapabilitySetV1 {
    GuestCapabilitySetV1 {
        screen_capture,
        pointer_input: input,
        keyboard_input: input,
        accessibility_tree: false,
        app_registry: GUEST_APPS
            .iter()
            .filter(|(app_id, path)| {
                config.allowed_apps.iter().any(|allowed| allowed == app_id)
                    && executable_present(path)
            })
            .map(|(app_id, _)| (*app_id).to_owned())
            .collect(),
    }
}

pub(super) fn launch_allowed_app(app_id: &str) -> Result<(), GuestProtocolErrorV1> {
    let (_, path) = GUEST_APPS
        .iter()
        .find(|(known_id, _)| *known_id == app_id)
        .ok_or(GuestProtocolErrorV1::UnsupportedCapability)?;
    if !trusted_executable_present(path) {
        return Err(GuestProtocolErrorV1::UnsupportedCapability);
    }
    let mut command = Command::new(path);
    match app_id {
        "chromium" => {
            command.args([
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-sync",
                "--disable-background-networking",
                "--user-data-dir=/run/koolade-computer-use-agent/chromium-profile",
                "--disk-cache-dir=/run/koolade-computer-use-agent/chromium-cache",
                "about:blank",
            ]);
        }
        "file-manager" => {
            command.arg("--no-desktop");
        }
        "text-editor" => {
            command.arg("--new-window");
        }
        "test-gui-fixture" => {}
        _ => return Err(GuestProtocolErrorV1::UnsupportedCapability),
    }
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/run/koolade-computer-use-agent")
        .env("DISPLAY", ":0")
        .env("XAUTHORITY", "/home/koolade/.Xauthority")
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|_| GuestProtocolErrorV1::UnsupportedCapability)
}

fn trusted_executable_present(path: &str) -> bool {
    let Ok(path) = Path::new(path).canonicalize() else {
        return false;
    };
    if !path.starts_with("/usr") {
        return false;
    }
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    metadata.is_file()
        && metadata.uid() == 0
        && metadata.mode() & 0o022 == 0
        && metadata.mode() & 0o111 != 0
}

pub(super) fn readable(fd: i32, timeout: Duration) -> Result<bool, GuestProtocolErrorV1> {
    let mut descriptor = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let millis = timeout.as_millis().min(i32::MAX as u128) as i32;
    let result = unsafe { libc::poll(&mut descriptor, 1, millis) };
    if result < 0 || descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        return Err(GuestProtocolErrorV1::Io);
    }
    Ok(result > 0 && descriptor.revents & libc::POLLIN != 0)
}

pub(super) fn write_exit_notice(
    port: &mut impl Write,
    config: &GuestBootConfigV1,
    handshake: &GuestHandshakeCompleteV1,
    sequence: u64,
    request: &GuestEnvelopeV1,
    error: GuestProtocolErrorV1,
) -> Result<(), GuestProtocolErrorV1> {
    let reason = match error {
        GuestProtocolErrorV1::UnsupportedCapability => GuestExitReasonV1::UnsupportedOperation,
        GuestProtocolErrorV1::Io => GuestExitReasonV1::GuestInteractionFailed,
        _ => GuestExitReasonV1::InternalProtocolFailure,
    };
    let exit = super::protocol::envelope(
        config,
        GuestEnvelopeContextV1 {
            sender: GuestSenderV1::Guest,
            sequence,
            request_id: request.request_id.clone(),
            action_id: request.action_id.clone(),
            deadline_unix_ms: request.deadline_unix_ms,
            capability_id: request.capability_id.clone(),
        },
        GuestMessageV1::GuestExit { reason },
    );
    write_frame(
        port,
        &AuthenticatedGuestEnvelopeV1::sign(exit, handshake.channel_key.expose())?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_app_registry_intersects_signed_allowlist_with_fixed_catalog() {
        let config = GuestBootConfigV1 {
            schema_version: 1,
            session_id: "00000000-0000-4000-8000-000000000001".into(),
            worker_id: "00000000-0000-4000-8000-000000000002".into(),
            lease_generation: 1,
            initial_action_sequence: 0,
            expires_at_unix_ms: 60_000,
            boot_id: "00000000-0000-4000-8000-000000000003".into(),
            secret_hex: "11".repeat(32),
            allowed_apps: vec![
                "chromium".into(),
                "file-manager".into(),
                "/bin/sh".into(),
                "unknown-app".into(),
            ],
        };
        let caps = guest_capabilities_with(&config, |_| true, true, true);
        assert_eq!(caps.app_registry, ["chromium", "file-manager"]);
        assert!(!caps.app_registry.iter().any(|app| app.starts_with('/')));
    }
}
