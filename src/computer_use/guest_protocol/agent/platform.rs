use crate::computer_use::guest_protocol::{
    AuthenticatedGuestEnvelopeV1, GuestBootConfigV1, GuestCapabilitySetV1, GuestEnvelopeContextV1,
    GuestEnvelopeV1, GuestExitReasonV1, GuestMessageV1, GuestProtocolErrorV1, GuestSenderV1,
    auth::GuestHandshakeCompleteV1, frame::write_frame,
};
use std::{
    io::Write,
    process::{Command, Stdio},
    time::Duration,
};

pub(super) fn guest_capabilities(config: &GuestBootConfigV1) -> GuestCapabilitySetV1 {
    GuestCapabilitySetV1 {
        screen_capture: true,
        pointer_input: true,
        keyboard_input: true,
        accessibility_tree: false,
        app_registry: config
            .allowed_apps
            .iter()
            .filter(|app| app.as_str() == "chromium")
            .cloned()
            .collect(),
    }
}

pub(super) fn launch_chromium() -> Result<(), GuestProtocolErrorV1> {
    Command::new("/usr/bin/chromium")
        .args([
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-sync",
            "--disable-background-networking",
            "--user-data-dir=/run/koolade-computer-use-agent/chromium-profile",
            "--disk-cache-dir=/run/koolade-computer-use-agent/chromium-cache",
            "about:blank",
        ])
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
