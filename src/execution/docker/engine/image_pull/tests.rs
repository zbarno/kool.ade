use std::sync::mpsc;

use super::{ImagePullControl, MAX_REPORTED_PULL_BYTES};

#[test]
fn progress_updates_are_bounded_and_sanitized() {
    let (sender, receiver) = mpsc::sync_channel(256);
    let control = ImagePullControl::new(sender);
    for index in 0..512 {
        control
            .record_progress(
                Some(&format!("layer-{}", index % 8)),
                Some(index % 101),
                Some(100),
            )
            .unwrap();
    }

    let progress = receiver.try_iter().collect::<Vec<_>>();
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
                (Some(current), Some(total)) =>
                    current <= total && total <= MAX_REPORTED_PULL_BYTES,
                (None, None) => item.percent.is_none(),
                _ => false,
            })
    );
}

#[test]
fn progress_without_layer_identity_is_indeterminate() {
    let (sender, receiver) = mpsc::sync_channel(4);
    let control = ImagePullControl::new(sender);
    control.record_progress(None, Some(10), Some(20)).unwrap();
    let progress = receiver.try_iter().collect::<Vec<_>>();
    assert_eq!(
        progress,
        vec![super::ImagePullProgress {
            current_bytes: None,
            total_bytes: None,
            percent: None,
        }]
    );
}

#[test]
fn full_progress_channel_does_not_consume_the_successful_event_budget() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let control = ImagePullControl::new(sender);
    control
        .record_progress(Some("layer"), Some(10), Some(100))
        .unwrap();
    control
        .record_progress(Some("layer"), Some(20), Some(100))
        .unwrap();
    let first = receiver.try_recv().unwrap();
    assert_eq!(first.current_bytes, Some(10));

    control
        .record_progress(Some("layer"), Some(30), Some(100))
        .unwrap();
    let later = receiver.try_recv().unwrap();
    assert_eq!(later.current_bytes, Some(30));
}

#[test]
fn aggregate_reported_size_cannot_exceed_the_bound() {
    let (sender, _receiver) = mpsc::sync_channel(1);
    let control = ImagePullControl::new(sender);
    let total_per_layer = (MAX_REPORTED_PULL_BYTES / 64 + 1) as i64;
    for index in 0..63 {
        control
            .record_progress(
                Some(&format!("layer-{index}")),
                Some(0),
                Some(total_per_layer),
            )
            .unwrap();
    }
    let error = control
        .record_progress(Some("layer-63"), Some(0), Some(total_per_layer))
        .unwrap_err();
    assert_eq!(
        error.code,
        crate::execution::docker::discovery::PrerequisiteCode::ImageUnavailable
    );
}

#[test]
fn bounded_bootstrap_rejects_zero_total_progress() {
    let (sender, _receiver) = mpsc::sync_channel(1);
    let control = ImagePullControl::new(sender);
    control.restrict_reported_bytes(1024);
    let error = control
        .record_progress(Some("layer"), Some(0), Some(0))
        .unwrap_err();
    assert_eq!(
        error.code,
        crate::execution::docker::discovery::PrerequisiteCode::ImageUnavailable
    );
}

#[test]
fn bounded_bootstrap_does_not_clear_unmeasured_transfer_on_retry() {
    let (sender, _receiver) = mpsc::sync_channel(1);
    let control = ImagePullControl::new(sender);
    control.restrict_reported_bytes(1024);
    control
        .record_unmeasured_layer(Some("layer"), false)
        .unwrap();

    let error = control
        .record_unmeasured_layer(Some("layer"), true)
        .unwrap_err();

    assert_eq!(
        error.code,
        crate::execution::docker::discovery::PrerequisiteCode::ImageUnavailable
    );
}

#[test]
fn clones_share_cancellation_state() {
    let (sender, _receiver) = mpsc::sync_channel(1);
    let control = ImagePullControl::new(sender);
    let worker_control = control.clone();
    control.cancel();
    assert!(worker_control.is_cancelled());
}
