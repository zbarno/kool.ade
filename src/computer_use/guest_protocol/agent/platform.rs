use crate::computer_use::guest_protocol::{
    AuthenticatedGuestEnvelopeV1, GuestBootConfigV1, GuestCapabilitySetV1, GuestEnvelopeContextV1,
    GuestEnvelopeV1, GuestExitReasonV1, GuestMessageV1, GuestProtocolErrorV1, GuestSenderV1,
    auth::GuestHandshakeCompleteV1, frame::write_frame,
};
use std::{io::Write, os::unix::fs::MetadataExt, path::Path, time::Duration};

const GUEST_APPS: &[(&str, &str)] = &[
    ("chromium", "/usr/bin/chromium"),
    ("chromium-research", "/usr/bin/chromium"),
    ("file-manager", "/usr/bin/thunar"),
    ("text-editor", "/usr/bin/mousepad"),
    (
        "test-gui-fixture",
        "/usr/lib/koolade/computer-use-test-fixture",
    ),
];

pub(super) fn guest_capabilities(config: &GuestBootConfigV1) -> GuestCapabilitySetV1 {
    let (screen_capture, input) = super::x11::capabilities();
    guest_capabilities_with(
        config,
        trusted_executable_present,
        crate::computer_use::apps::guest_helper::app_management_available(),
        crate::computer_use::apps::guest_helper::installed_app_ids(),
        screen_capture,
        input,
    )
}

fn guest_capabilities_with(
    config: &GuestBootConfigV1,
    executable_present: impl Fn(&str) -> bool,
    app_management: bool,
    catalog_installed: Vec<String>,
    screen_capture: bool,
    input: bool,
) -> GuestCapabilitySetV1 {
    GuestCapabilitySetV1 {
        screen_capture,
        pointer_input: input,
        keyboard_input: input,
        accessibility_tree: false,
        app_registry: {
            let mut apps = GUEST_APPS
                .iter()
                .filter(|(app_id, path)| {
                    config.allowed_apps.iter().any(|allowed| allowed == app_id)
                        && executable_present(path)
                })
                .map(|(app_id, _)| (*app_id).to_owned())
                .collect::<std::collections::BTreeSet<_>>();
            apps.extend(
                catalog_installed
                    .into_iter()
                    .filter(|app| config.allowed_apps.iter().any(|allowed| allowed == app)),
            );
            apps.into_iter().collect()
        },
        app_management,
        staged_task_app_launch:
            crate::computer_use::apps::guest_helper::staged_task_app_launch_available(),
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
    crate::computer_use::apps::guest_helper::launch(app_id)
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
                "chromium-research".into(),
                "file-manager".into(),
                "/bin/sh".into(),
                "unknown-app".into(),
            ],
        };
        let caps = guest_capabilities_with(&config, |_| true, false, vec![], true, true);
        assert_eq!(
            caps.app_registry,
            ["chromium", "chromium-research", "file-manager"]
        );
        assert!(!caps.app_management);
        assert!(!caps.app_registry.iter().any(|app| app.starts_with('/')));
    }
}
