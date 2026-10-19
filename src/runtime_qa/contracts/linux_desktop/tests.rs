use super::{
    LinuxDesktopCapabilityV1, LinuxDesktopProbeV1, LinuxDesktopSessionV1, LinuxDesktopWindowV1,
};
use crate::execution::docker::images::ImageLock;
use crate::runtime_qa::QaDriverActionV1;

#[test]
fn active_window_actions_require_one_current_owned_window() {
    let session = LinuxDesktopSessionV1 {
        qa_run_id: "11111111-1111-4111-8111-111111111111".into(),
        display_width: 1440,
        display_height: 900,
        windows: vec![LinuxDesktopWindowV1 {
            window_id: "2097153".into(),
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        }],
    };
    let click = QaDriverActionV1::Click {
        window_id: "active".into(),
        x: 20,
        y: 30,
    };
    assert!(session.validate_action(&click).is_ok());

    let mut ambiguous = session.clone();
    ambiguous.windows.push(LinuxDesktopWindowV1 {
        window_id: "2097154".into(),
        x: 650,
        y: 0,
        width: 640,
        height: 480,
    });
    assert!(ambiguous.validate_action(&click).is_err());
}

#[test]
fn desktop_capability_uses_the_locked_desktop_image_profile() {
    let mut lock = ImageLock::bundled().unwrap();
    lock.profiles
        .get_mut("browser_qa")
        .unwrap()
        .iter_mut()
        .for_each(|variant| variant.runtime_enabled = false);
    lock.profiles
        .get_mut("desktop_qa")
        .unwrap()
        .iter_mut()
        .for_each(|variant| {
            variant.runtime_enabled = true;
            variant
                .tool_versions
                .insert("chromium".into(), "143.0.7499.4".into());
            variant
                .tool_versions
                .insert("dbus".into(), "1.14.10-4ubuntu4.1".into());
        });
    let probe = LinuxDesktopProbeV1 {
        schema_version: 1,
        qa_run_id: "11111111-1111-4111-8111-111111111111".into(),
        task_uid: "22222222-2222-4222-8222-222222222222".into(),
        image_digest: "a".repeat(64),
        artifact_sha256: "b".repeat(64),
        executable_sha256: "b".repeat(64),
        display_width: 1440,
        display_height: 900,
        dpi: 96,
        private_xvfb_ready: true,
        llvmpipe_ready: true,
        input_driver_ready: true,
        screenshot_capture_ready: true,
        isolated_home_ready: true,
        process_scope_ready: true,
        host_display_mounted: false,
        host_wayland_mounted: false,
        host_dbus_mounted: false,
        host_xauthority_mounted: false,
        host_input_mounted: false,
        host_gpu_mounted: false,
        host_audio_mounted: false,
        evidence_refs: ["image", "display", "isolation"]
            .map(|name| format!("qa-evidence/11111111-1111-4111-8111-111111111111/{name}.json"))
            .into(),
        recorded_at_unix_ms: 1,
    };

    assert!(matches!(
        probe.assess_with_image_lock(&lock).unwrap(),
        LinuxDesktopCapabilityV1::Ready { .. }
    ));
}
