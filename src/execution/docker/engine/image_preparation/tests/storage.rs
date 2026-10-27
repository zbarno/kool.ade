use super::*;
use std::sync::mpsc;

#[test]
fn insufficient_fresh_image_storage_blocks_the_fake_pull() {
    let image = locked_image();
    let api = Arc::new(fake_api("synthetic-daemon", PullBehavior::Progress));
    let engine = engine(api.clone());
    let authorization = authorization(&engine, &image, "synthetic-daemon");
    api.snapshot.lock().unwrap().disk_available_bytes = Some(1024);
    let (progress_tx, _progress_rx) = mpsc::sync_channel(16);
    let control = ImagePullControl::new(progress_tx);

    let error = runtime()
        .block_on(engine.prepare_approved_image(&image, &authorization, &control))
        .unwrap_err();

    assert_eq!(error.code, PrerequisiteCode::InsufficientDisk);
    assert_eq!(api.pulls.load(Ordering::Acquire), 0);
}

#[test]
fn unknown_fresh_image_storage_blocks_the_fake_pull() {
    let image = locked_image();
    let api = Arc::new(fake_api("synthetic-daemon", PullBehavior::Progress));
    let engine = engine(api.clone());
    let authorization = authorization(&engine, &image, "synthetic-daemon");
    api.snapshot.lock().unwrap().disk_available_bytes = None;
    let (progress_tx, _progress_rx) = mpsc::sync_channel(16);
    let control = ImagePullControl::new(progress_tx);

    let error = runtime()
        .block_on(engine.prepare_approved_image(&image, &authorization, &control))
        .unwrap_err();

    assert_eq!(error.code, PrerequisiteCode::InsufficientDisk);
    assert_eq!(api.pulls.load(Ordering::Acquire), 0);
}
