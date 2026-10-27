use std::sync::{Arc, atomic::Ordering, mpsc};

use futures_util::stream;

use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    engine::ImagePullControl,
};

use super::super::retry as pull_retry;

#[tokio::test]
async fn engine_identity_change_between_attempts_prevents_retry_stream_creation() {
    let (progress_tx, _progress_rx) = mpsc::sync_channel(4);
    let control = ImagePullControl::new(progress_tx);
    let identity_checks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let verify_checks = identity_checks.clone();
    let mut verify_engine = move || {
        let checks = verify_checks.clone();
        async move {
            if checks.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(())
            } else {
                Err(PrerequisiteFailure {
                    code: PrerequisiteCode::UntrustedEndpoint,
                    message: "synthetic daemon identity changed".into(),
                    retryable: false,
                })
            }
        }
    };
    let stream_attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let create_attempts = stream_attempts.clone();

    let error = pull_retry::pull_with_retries(
        &mut verify_engine,
        || {
            create_attempts.fetch_add(1, Ordering::SeqCst);
            stream::iter(vec![Err(bollard::errors::Error::RequestTimeoutError)])
        },
        &control,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, PrerequisiteCode::UntrustedEndpoint);
    assert_eq!(identity_checks.load(Ordering::SeqCst), 2);
    assert_eq!(stream_attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn insufficient_fresh_storage_between_attempts_prevents_retry_stream_creation() {
    let (progress_tx, _progress_rx) = mpsc::sync_channel(4);
    let control = ImagePullControl::new(progress_tx);
    let capacity_checks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let verify_checks = capacity_checks.clone();
    let mut verify_capacity = move || {
        let checks = verify_checks.clone();
        async move {
            if checks.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(())
            } else {
                Err(PrerequisiteFailure {
                    code: PrerequisiteCode::InsufficientDisk,
                    message: "synthetic disk space was consumed by the first attempt".into(),
                    retryable: false,
                })
            }
        }
    };
    let stream_attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let create_attempts = stream_attempts.clone();

    let error = pull_retry::pull_with_retries(
        &mut verify_capacity,
        || {
            create_attempts.fetch_add(1, Ordering::SeqCst);
            stream::iter(vec![Err(bollard::errors::Error::RequestTimeoutError)])
        },
        &control,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, PrerequisiteCode::InsufficientDisk);
    assert_eq!(capacity_checks.load(Ordering::SeqCst), 2);
    assert_eq!(stream_attempts.load(Ordering::SeqCst), 1);
}
