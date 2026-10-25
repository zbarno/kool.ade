use super::KooladeApp;

/// Production boundary: the regular app tick must collect image-preparation completion and
/// schedule fresh machine readiness even when Task Details is closed. A controller-only test
/// cannot prove that lifecycle integration; this uses a queued fake completion and a parked
/// setup probe, with no Docker, filesystem, or network dependency and low setup cost.
#[test]
fn tick_refreshes_setup_after_image_preparation_without_open_details() {
    let mut app = KooladeApp::default();
    let (release_probe, wait_for_probe) = std::sync::mpsc::channel();
    app.setup_probe = Some(std::thread::spawn(move || {
        let _ = wait_for_probe.recv();
        crate::app::root::setup_attention::ProbeOutput::Scanned(
            crate::app::setup_attention::SetupScanResult {
                generation: 0,
                issue: None,
                snapshot: None,
                endpoint_fingerprint: None,
                daemon_identity_fingerprint: None,
                docker_readiness_fingerprint: None,
                selected_provider: None,
                cancelled: true,
            },
        )
    }));
    app.setup_image_preparation.queue_success_for_test();

    app.tick(0.0, &egui::Context::default());

    assert_eq!(app.setup_refresh_pending, Some(true));
    release_probe.send(()).unwrap();
    app.setup_probe.take().unwrap().join().unwrap();
}
