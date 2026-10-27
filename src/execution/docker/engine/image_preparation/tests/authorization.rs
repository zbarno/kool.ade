use super::*;
use std::sync::mpsc;

#[test]
fn changed_engine_identity_is_rejected_before_the_fake_pull() {
    let image = locked_image();
    let api = Arc::new(fake_api("current-daemon", PullBehavior::Progress));
    let engine = engine(api.clone());
    let old_authorization = authorization(&engine, &image, "previous-daemon");
    let (progress_tx, _progress_rx) = mpsc::sync_channel(16);
    let control = ImagePullControl::new(progress_tx);

    let error = runtime()
        .block_on(engine.prepare_approved_image(&image, &old_authorization, &control))
        .unwrap_err();

    assert_eq!(error.code, PrerequisiteCode::UntrustedEndpoint);
    assert_eq!(api.pulls.load(Ordering::Acquire), 0);
}

#[test]
fn mismatched_opaque_approval_is_rejected_before_the_fake_pull() {
    let image = locked_image();
    let api = Arc::new(fake_api("synthetic-daemon", PullBehavior::Progress));
    let engine = engine(api.clone());
    let snapshot = snapshot("synthetic-daemon");
    let engine_fingerprint =
        super::super::super::approval_fingerprint(engine.endpoint(), &snapshot, &image).unwrap();
    let engine_identity = crate::app::bootstrap_approval::engine_identity_fingerprint(
        &engine.endpoint().fingerprint(),
        &snapshot.daemon_id,
    )
    .unwrap();
    let authorization = ApprovedAction::for_test(
        "rust",
        &format!("setup:images:rust:amd64:{engine_identity}"),
        &image,
        &engine_fingerprint,
        &engine_identity,
    );
    let (progress_tx, _progress_rx) = mpsc::sync_channel(16);
    let control = ImagePullControl::new(progress_tx);

    let error = runtime()
        .block_on(engine.prepare_approved_image(&image, &authorization, &control))
        .unwrap_err();

    assert_eq!(error.code, PrerequisiteCode::ImageDigestMismatch);
    assert_eq!(api.pulls.load(Ordering::Acquire), 0);
}
