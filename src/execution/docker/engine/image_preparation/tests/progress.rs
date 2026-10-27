use super::*;
use std::sync::mpsc;

#[test]
fn approved_pull_reports_bounded_progress_through_the_fake_api() {
    let image = locked_image();
    let api = Arc::new(fake_api("synthetic-daemon", PullBehavior::Progress));
    let engine = engine(api.clone());
    let authorization = authorization(&engine, &image, "synthetic-daemon");
    let (progress_tx, progress_rx) = mpsc::sync_channel(256);
    let control = ImagePullControl::new(progress_tx);

    runtime()
        .block_on(engine.prepare_approved_image(&image, &authorization, &control))
        .unwrap();

    let progress = progress_rx.try_iter().collect::<Vec<_>>();
    assert!(!progress.is_empty());
    assert!(progress.len() <= 128);
    assert!(
        progress
            .iter()
            .all(|item| item.percent.is_some_and(|p| p <= 100))
    );
    assert!(
        progress
            .iter()
            .all(|item| match (item.current_bytes, item.total_bytes) {
                (Some(current), Some(total)) => current <= total,
                (None, None) => item.percent.is_none(),
                _ => false,
            })
    );
    assert_eq!(api.pulls.load(Ordering::Acquire), 1);
}

#[test]
fn cancellation_is_observed_during_a_fake_pull() {
    let image = locked_image();
    let api = Arc::new(fake_api(
        "synthetic-daemon",
        PullBehavior::WaitForCancellation,
    ));
    let engine = engine(api.clone());
    let authorization = authorization(&engine, &image, "synthetic-daemon");
    let (progress_tx, _progress_rx) = mpsc::sync_channel(16);
    let control = ImagePullControl::new(progress_tx);
    let runtime = runtime();

    let result = runtime.block_on(async {
        let pull_control = control.clone();
        let pull = tokio::spawn(async move {
            engine
                .prepare_approved_image(&image, &authorization, &pull_control)
                .await
        });
        while api.pulls.load(Ordering::Acquire) == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        control.cancel();
        pull.await.unwrap()
    });

    assert_eq!(result.unwrap_err().code, PrerequisiteCode::ImageUnavailable);
    assert_eq!(api.pulls.load(Ordering::Acquire), 1);
}
